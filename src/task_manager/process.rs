//! Child-process supervision for blocking and asynchronous tracked tasks.
use std::{
    io,
    process::{Child, ExitStatus},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

pub struct ChildGuard(Child);
impl ChildGuard {
    pub fn new(child: Child) -> Self {
        Self(child)
    }
    pub fn wait(
        &mut self,
        cancel: &CancellationToken,
        timeout: Duration,
    ) -> io::Result<ExitStatus> {
        let start = Instant::now();
        loop {
            if let Some(status) = self.0.try_wait()? {
                return Ok(status);
            }
            if cancel.is_cancelled() {
                return Err(io::ErrorKind::Interrupted.into());
            }
            if start.elapsed() >= timeout {
                return Err(io::ErrorKind::TimedOut.into());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(Some(_))) {
            return;
        }
        if let Err(error) = self.0.kill() {
            eprintln!("Cannot stop child during cleanup: {error}");
        }
        if let Err(error) = self.0.wait() {
            eprintln!("Cannot reap child during cleanup: {error}");
        }
    }
}

/// Capture a child and reap it on cancellation or timeout. On Unix, its process
/// group is terminated together. Call inside a tracked task; do not abort that
/// task while it is terminating the child.
pub async fn output(
    command: &mut tokio::process::Command,
    cancel: &CancellationToken,
    timeout: Duration,
) -> io::Result<std::process::Output> {
    use tokio::io::AsyncReadExt;
    if cancel.is_cancelled() {
        return Err(io::ErrorKind::Interrupted.into());
    }
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let wait = async {
        let reason = tokio::select! {
            status = child.wait() => return status,
            _ = cancel.cancelled() => io::ErrorKind::Interrupted,
            _ = tokio::time::sleep(timeout) => io::ErrorKind::TimedOut,
        };
        #[cfg(unix)]
        if let Some(pid) = child.id() {
            // Tokio clears id after reaping; never signal an already-reaped PID.
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        child.start_kill()?;
        child.wait().await?;
        Err(reason.into())
    };
    let mut out = Vec::new();
    let mut err = Vec::new();
    let (status, read_out, read_err) = tokio::join!(
        wait,
        stdout.read_to_end(&mut out),
        stderr.read_to_end(&mut err)
    );
    Ok(std::process::Output {
        status: status?,
        stdout: {
            read_out?;
            out
        },
        stderr: {
            read_err?;
            err
        },
    })
}
