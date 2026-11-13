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

const MAX_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;

async fn read_bounded<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    limit: u64,
) -> io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;

    let mut buf = Vec::new();
    (&mut reader).take(limit).read_to_end(&mut buf).await?;
    if buf.len() as u64 == limit {
        // Drain excess bytes into a fixed buffer so the child neither stalls on
        // a full pipe nor hits SIGPIPE while buffered output stays bounded.
        let mut discard = [0u8; 8192];
        while reader.read(&mut discard).await? > 0 {}
    }
    Ok(buf)
}

async fn terminate_child(child: &mut tokio::process::Child) -> io::Result<()> {
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
    Ok(())
}

/// Capture a child and reap it on cancellation or timeout. On Unix, its process
/// group is terminated together. Call inside a tracked task; do not abort that
/// task while it is terminating the child.
pub async fn output(
    command: &mut tokio::process::Command,
    cancel: &CancellationToken,
    timeout: Duration,
) -> io::Result<std::process::Output> {
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
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let read_out = read_bounded(stdout, MAX_OUTPUT_BYTES);
    let read_err = read_bounded(stderr, MAX_OUTPUT_BYTES);
    tokio::pin!(read_out, read_err);

    let mut status: Option<ExitStatus> = None;
    let mut out_result: Option<io::Result<Vec<u8>>> = None;
    let mut err_result: Option<io::Result<Vec<u8>>> = None;

    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);

    while status.is_none() || out_result.is_none() || err_result.is_none() {
        if cancel.is_cancelled() {
            if status.is_none() {
                terminate_child(&mut child).await?;
            }
            return Err(io::ErrorKind::Interrupted.into());
        }
        tokio::select! {
            res = child.wait(), if status.is_none() => {
                status = Some(res?);
            }
            res = &mut read_out, if out_result.is_none() => {
                out_result = Some(res);
            }
            res = &mut read_err, if err_result.is_none() => {
                err_result = Some(res);
            }
            _ = cancel.cancelled() => {
                if status.is_none() {
                    terminate_child(&mut child).await?;
                }
                return Err(io::ErrorKind::Interrupted.into());
            }
            _ = &mut deadline => {
                if status.is_none() {
                    terminate_child(&mut child).await?;
                }
                return Err(io::ErrorKind::TimedOut.into());
            }
        }
    }

    Ok(std::process::Output {
        status: status.expect("loop exits only once status is collected"),
        stdout: out_result.expect("loop exits only once stdout is collected")?,
        stderr: err_result.expect("loop exits only once stderr is collected")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    fn cleanup_pid_file(path: &std::path::Path) {
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(pid) = text.trim().parse::<i32>()
        {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn output_timeout_returns_promptly_when_setsid_grandchild_holds_stdout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pid_file = dir.path().join("grandchild.pid");
        let mut command = tokio::process::Command::new("sh");
        command.args([
            "-c",
            "setsid sh -c 'echo $$ > \"$1\"; exec sleep 30' _ \"$1\" & exec sleep 30",
            "_",
            pid_file.to_str().expect("utf8 path"),
        ]);

        let started = Instant::now();
        let result = output(
            &mut command,
            &CancellationToken::new(),
            Duration::from_millis(150),
        )
        .await;
        let elapsed = started.elapsed();
        cleanup_pid_file(&pid_file);

        assert_eq!(
            result.expect_err("expected timeout").kind(),
            io::ErrorKind::TimedOut
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "timed out command hung for {elapsed:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn output_cancel_returns_promptly_when_setsid_grandchild_holds_stdout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pid_file = dir.path().join("grandchild.pid");
        let cancel = CancellationToken::new();
        let child_cancel = cancel.clone();
        let pid_arg = pid_file.to_str().expect("utf8 path").to_owned();

        let task = tokio::spawn(async move {
            let mut command = tokio::process::Command::new("sh");
            command.args([
                "-c",
                "setsid sh -c 'echo $$ > \"$1\"; exec sleep 30' _ \"$1\" & exec sleep 30",
                "_",
                &pid_arg,
            ]);
            output(&mut command, &child_cancel, Duration::from_secs(30)).await
        });

        tokio::time::timeout(Duration::from_secs(5), async {
            while std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|s| s.trim().parse::<i32>().ok())
                .is_none()
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("grandchild pid should appear");

        let started = Instant::now();
        cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("cancelled output should return promptly")
            .expect("join task");
        let elapsed = started.elapsed();
        cleanup_pid_file(&pid_file);

        assert_eq!(
            result.expect_err("expected interruption").kind(),
            io::ErrorKind::Interrupted
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "cancelled command hung for {elapsed:?}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn output_caps_oversized_stdout_cleanly() {
        let mut command = tokio::process::Command::new("sh");
        command.args(["-c", "dd if=/dev/zero bs=65536 count=272 2>/dev/null"]);
        let out = output(
            &mut command,
            &CancellationToken::new(),
            Duration::from_secs(10),
        )
        .await
        .expect("oversized stdout should succeed cleanly");
        assert!(out.status.success());
        assert_eq!(out.stdout.len(), MAX_OUTPUT_BYTES as usize);
        assert!(out.stderr.is_empty());
    }
}
