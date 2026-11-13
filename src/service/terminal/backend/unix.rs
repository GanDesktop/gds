use std::{
    collections::BTreeSet,
    io::{self, Read, Write},
    os::fd::{BorrowedFd, RawFd},
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use alacritty_terminal::vte::ansi::Processor;
use nix::{
    errno::Errno,
    fcntl::{FcntlArg, OFlag, fcntl},
    sys::signal::{Signal, killpg},
    unistd::{Pid, getsid},
};
use portable_pty::{Child, ExitStatus, MasterPty};
use tokio::{
    io::{Interest, unix::AsyncFd},
    signal::unix::{SignalKind, signal},
};

use super::{
    super::session::{Completion, Error, Shared, Size, SpawnOptions, Status, lock},
    shell::Command,
};

const READ_CHUNK: usize = 4096;
const IO_BUDGET: usize = 64 * 1024;
// GPUI's quit fallback has a 200 ms budget; normal exit waits after closing the GUI.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(50);
const FINAL_DRAIN_BUDGET: usize = 256 * 1024;

pub(crate) struct Backend {
    master: Box<dyn MasterPty + Send>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    child: Option<Box<dyn Child + Send + Sync>>,
    pid: Option<u32>,
    directory_commands: bool,
    fd: RawFd,
    reaped: bool,
    // Fields drop in declaration order: signal completion AFTER all PTY handles.
    completion: Option<Completion>,
}

struct Pending {
    bytes: Vec<u8>,
    offset: usize,
    response: bool,
}

impl Backend {
    pub(crate) fn open(options: &SpawnOptions, size: Size) -> Result<Self, Error> {
        let command = Command::prepare(options)?;
        let pair = portable_pty::native_pty_system().openpty(size.pty_size())?;
        let fd = pair
            .master
            .as_raw_fd()
            .ok_or_else(|| anyhow::anyhow!("PTY has no Unix descriptor"))?;
        // The master remains owned by Backend until after all uses of this fd.
        let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
        let flags = fcntl(borrowed, FcntlArg::F_GETFL).map_err(anyhow::Error::from)?;
        fcntl(
            borrowed,
            FcntlArg::F_SETFL(OFlag::from_bits_retain(flags) | OFlag::O_NONBLOCK),
        )
        .map_err(anyhow::Error::from)?;
        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let slave_path = pair
            .master
            .tty_name()
            .ok_or_else(|| anyhow::anyhow!("PTY has no Unix slave device path"))?;
        let directory_commands = command.directory_commands;
        let child: Box<dyn Child + Send + Sync> =
            Box::new(command.spawn(&slave_path).map_err(anyhow::Error::from)?);
        let pid = child.process_id();
        drop(pair.slave);
        Ok(Self {
            master: pair.master,
            reader,
            writer,
            child: Some(child),
            pid,
            directory_commands,
            fd,
            reaped: false,
            completion: None,
        })
    }

    pub(crate) fn with_completion(mut self, completion: Completion) -> Self {
        self.completion = Some(completion);
        self
    }

    pub(crate) fn run(mut self, shared: Arc<Shared>, tasks: crate::service::task_manager::Scope) {
        let mut parser = Processor::new();
        let cancel = tasks.token();
        let result = match tasks.block_on_named("Read and write terminal PTY", async {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => Ok(None),
                result = self.io_loop(&shared, &mut parser) => result,
            }
        }) {
            Ok(result) => result,
            Err(crate::service::task_manager::Error::Closed(_)) => Ok(None),
            Err(error) => Err(error.into()),
        };
        if let Err(error) = &result {
            shared.state.fail(error);
        }
        shared.state.shutdown.store(true, Ordering::Release);
        let cleanup = self.shutdown();
        if let Err(error) = &cleanup {
            shared.state.fail(error);
        }
        // Preserve final output before publishing the final status/closing updates.
        if let Err(error) = self.read_output(&shared, &mut parser, FINAL_DRAIN_BUDGET) {
            if error.kind() != io::ErrorKind::WouldBlock {
                shared.state.fail(error);
            }
        }
        parser.stop_sync(&mut *shared.term.lock());
        {
            let mut control = lock(&shared.state.control);
            control.directory_change.take();
            control.input.clear();
            control.responses.clear();
            control.color_queries.clear();
            control.input_bytes = 0;
            control.response_bytes = 0;
        }
        let mut metadata = lock(&shared.state.metadata);
        metadata.status = if let Some(error) = metadata.error.take() {
            Status::Failed(error)
        } else {
            match result.ok().flatten().or_else(|| cleanup.ok().flatten()) {
                Some(status) => Status::Exited {
                    code: status.exit_code(),
                    signal: status.signal().map(str::to_owned),
                },
                None => Status::Failed("The terminal process exited without a wait status".into()),
            }
        };
        drop(metadata);
        shared.state.notify();
        // Completion closes notifications and the quit barrier after fd drops.
    }

    async fn io_loop(
        &mut self,
        shared: &Shared,
        parser: &mut Processor,
    ) -> anyhow::Result<Option<ExitStatus>> {
        // SAFETY: Backend owns this open master for the whole registration lifetime;
        // io_loop drops the registration before run/Drop can release the PTY handles.
        let descriptor = unsafe { AsyncFd::register(self.fd) }?;
        // Subscribe before the first try_wait, including an already-exited child.
        let mut children = signal(SignalKind::child())?;
        let mut pending: Option<Pending> = None;
        loop {
            if shared.state.shutdown.load(Ordering::Acquire) {
                return Ok(None);
            }
            let resize = lock(&shared.state.control).resize.take();
            if let Some(size) = resize.filter(|size| lock(&shared.state.metadata).size != *size) {
                self.master.resize(size.pty_size())?;
                {
                    let mut term = shared.term.lock();
                    // Clamp history before widening/reflow can allocate more cells.
                    term.set_options(size.term_config(shared.scrollback));
                    term.resize(size);
                }
                lock(&shared.state.metadata).size = size;
                shared.state.notify();
            }
            self.change_directory(shared);
            match descriptor.try_io(Interest::WRITABLE, |_| {
                self.write_input(shared, &mut pending)
            }) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error.into()),
            }
            match descriptor.try_io(Interest::READABLE, |_| {
                self.read_output(shared, parser, IO_BUDGET)
            }) {
                // A final slave close can precede waitpid readiness; cleanup waits
                // the direct child rather than spinning on a permanently-ready EOF.
                Ok(0) => return Ok(None),
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error.into()),
            }
            if parser
                .sync_timeout()
                .sync_timeout()
                .is_some_and(|deadline| deadline <= Instant::now())
            {
                parser.stop_sync(&mut *shared.term.lock());
                Self::answer_color_queries(shared);
                shared.state.notify();
            }
            if let Some(child) = self.child.as_mut() {
                if let Some(status) = child.try_wait()? {
                    self.reaped = true;
                    self.child.take();
                    return Ok(Some(status));
                }
            }
            let wants_write = pending.is_some() || {
                let control = lock(&shared.state.control);
                !control.input.is_empty() || !control.responses.is_empty()
            };
            let interest = if wants_write {
                Interest::READABLE | Interest::WRITABLE
            } else {
                Interest::READABLE
            };
            let deadline = parser.sync_timeout().sync_timeout();
            enum Event {
                Ready(io::Result<()>),
                Control,
                Child(Option<()>),
                SyncTimeout,
            }
            let event = tokio::select! {
                ready = descriptor.ready(interest) => {
                    Event::Ready(ready.map(|mut ready| ready.retain_ready()))
                }
                _ = shared.state.wake.notified() => Event::Control,
                child = children.recv() => Event::Child(child),
                _ = async {
                    match deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
                        None => std::future::pending().await,
                    }
                } => Event::SyncTimeout,
            };
            match event {
                Event::Ready(result) => result?,
                Event::Child(None) => anyhow::bail!("Terminal child signal stream closed"),
                Event::Control | Event::Child(Some(())) | Event::SyncTimeout => {}
            }
        }
    }

    fn read_output(
        &mut self,
        shared: &Shared,
        parser: &mut Processor,
        budget: usize,
    ) -> io::Result<usize> {
        let mut bytes = [0; READ_CHUNK];
        let mut processed = 0;
        while processed < budget {
            match self.reader.read(&mut bytes) {
                Ok(0) => break,
                Ok(count) => {
                    // Retain this parser across reads, including split UTF-8 and CSI.
                    parser.advance(&mut *shared.term.lock(), &bytes[..count]);
                    Self::answer_color_queries(shared);
                    processed += count;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock && processed != 0 => break,
                // portable-pty maps Unix EIO to EOF, but keep this defensive.
                Err(error) if error.raw_os_error() == Some(nix::libc::EIO) => break,
                Err(error) => return Err(error),
            }
        }
        if processed != 0 {
            shared.state.notify();
        }
        // Budget exhaustion retains readiness; only an actual WouldBlock clears it.
        Ok(processed)
    }

    fn answer_color_queries(shared: &Shared) {
        loop {
            let query = lock(&shared.state.control).color_queries.pop_front();
            let Some(query) = query else {
                break;
            };
            if query.index >= alacritty_terminal::term::color::COUNT {
                continue;
            }
            let color = shared.term.lock().colors()[query.index]
                .unwrap_or_else(|| shared.state.palette.color(query.index));
            shared.state.respond((query.format)(color));
        }
    }

    fn change_directory(&self, shared: &Shared) {
        let mut control = lock(&shared.state.control);
        let Some(request) = control.directory_change.take() else {
            return;
        };
        let result = if !self.directory_commands {
            Err(Error::UnsupportedDirectoryCommand)
        } else {
            // An empty transport queue and foreground shell PID say nothing
            // about the shell's editing buffer. Without trusted integration,
            // injecting a cd plus Enter can submit an unconfirmed user command.
            Err(Error::ShellBusy)
        };
        // Only the caller receives this one-shot result; a dropped caller does
        // not turn a rejected command into a terminal process failure.
        let _ = request.result.send(result);
    }

    fn write_input(&mut self, shared: &Shared, pending: &mut Option<Pending>) -> io::Result<()> {
        let mut written = 0;
        while written < IO_BUDGET {
            if pending.is_none() {
                let mut control = lock(&shared.state.control);
                *pending = control
                    .responses
                    .pop_front()
                    .map(|bytes| Pending {
                        bytes,
                        offset: 0,
                        response: true,
                    })
                    .or_else(|| {
                        control.input.pop_front().map(|bytes| Pending {
                            bytes,
                            offset: 0,
                            response: false,
                        })
                    });
            }
            let Some(message) = pending.as_mut() else {
                break;
            };
            let end = message
                .bytes
                .len()
                .min(message.offset + IO_BUDGET - written);
            match self.writer.write(&message.bytes[message.offset..end]) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "PTY write returned zero",
                    ));
                }
                Ok(count) => {
                    message.offset += count;
                    written += count;
                    let mut control = lock(&shared.state.control);
                    if message.response {
                        control.response_bytes -= count;
                    } else {
                        control.input_bytes -= count;
                    }
                    if message.offset == message.bytes.len() {
                        *pending = None;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn process_groups(&self) -> io::Result<BTreeSet<i32>> {
        let mut groups = BTreeSet::new();
        let Some(pid) = self.pid.and_then(|pid| i32::try_from(pid).ok()) else {
            return Ok(groups);
        };
        if !self.reaped {
            groups.insert(pid);
        }
        if let Some(foreground) = self.master.process_group_leader() {
            // Never trust a stale foreground pgid after the last job has exited.
            if foreground > 0
                && getsid(Some(Pid::from_raw(foreground))).is_ok_and(|sid| sid.as_raw() == pid)
            {
                groups.insert(foreground);
            }
        }
        #[cfg(target_os = "linux")]
        {
            // Job-control shells create one process group per pipeline. Include
            // background/stopped jobs in this PTY session, not only the shell.
            for entry in std::fs::read_dir("/proc")? {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error),
                };
                if entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.parse::<u32>().ok())
                    .is_none()
                {
                    continue;
                }
                // A task can disappear after open as well as before it; procfs
                // then reports ESRCH instead of ENOENT while reading stat.
                let stat = match std::fs::read_to_string(entry.path().join("stat")) {
                    Ok(stat) => stat,
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
                        ) || error.raw_os_error() == Some(nix::libc::ESRCH) =>
                    {
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                // comm is parenthesized and may itself contain spaces or ')'.
                let Some((_, fields)) = stat.rsplit_once(") ") else {
                    continue;
                };
                let mut fields = fields.split_whitespace();
                let group = fields.nth(2).and_then(|value| value.parse::<i32>().ok());
                let session = fields.next().and_then(|value| value.parse::<i32>().ok());
                if session == Some(pid) {
                    if let Some(group) = group.filter(|group| *group > 0) {
                        groups.insert(group);
                    }
                }
            }
        }
        Ok(groups)
    }

    fn signal_groups(groups: &BTreeSet<i32>, signal: Signal) -> io::Result<()> {
        let mut first_error = None;
        for &group in groups {
            match killpg(Pid::from_raw(group), signal) {
                Ok(()) | Err(Errno::ESRCH) => {}
                Err(error) => {
                    first_error.get_or_insert(io::Error::from_raw_os_error(error as i32));
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    fn shutdown(&mut self) -> io::Result<Option<ExitStatus>> {
        let mut error = None;
        let mut reaped_status = None;
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.reaped = true;
                    self.child = None;
                    reaped_status = Some(status);
                }
                Ok(None) => {}
                Err(failure) => {
                    error.get_or_insert(failure);
                }
            }
        }
        let mut groups = match self.process_groups() {
            Ok(groups) => groups,
            Err(failure) => {
                error.get_or_insert(failure);
                self.pid
                    .filter(|_| !self.reaped)
                    .and_then(|pid| i32::try_from(pid).ok())
                    .into_iter()
                    .collect()
            }
        };
        if !groups.is_empty() {
            for signal in [Signal::SIGHUP, Signal::SIGCONT] {
                if let Err(failure) = Self::signal_groups(&groups, signal) {
                    error.get_or_insert(failure);
                }
            }
            // Only this worker waits; Drop/terminate on the UI never joins it.
            std::thread::sleep(SHUTDOWN_GRACE);
            match self.process_groups() {
                // Re-enumerate rather than signaling previously reaped pgids.
                Ok(current) => groups = current,
                Err(failure) => {
                    error.get_or_insert(failure);
                    // A previous job's pgid may now belong to another session.
                    // Only the unreaped direct child's id is still reserved.
                    groups = self
                        .pid
                        .filter(|_| !self.reaped)
                        .and_then(|pid| i32::try_from(pid).ok())
                        .into_iter()
                        .collect();
                }
            }
            if let Err(failure) = Self::signal_groups(&groups, Signal::SIGKILL) {
                error.get_or_insert(failure);
            }
        }
        let status = if let Some(mut child) = self.child.take() {
            // The direct child is always waited, even after errors and SIGHUP.
            let result = child.wait();
            self.reaped = result.is_ok();
            match result {
                Ok(status) => Some(status),
                Err(failure) => {
                    error.get_or_insert(failure);
                    None
                }
            }
        } else {
            reaped_status
        };
        error.map_or(Ok(status), Err)
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        if self.child.is_some() {
            // This also covers failure to create the I/O thread, on the caller's
            // background executor. Normal run() has already waited the child.
            if let Err(error) = self.shutdown() {
                eprintln!("Failed to reap terminal child: {error}");
            }
        }
    }
}
