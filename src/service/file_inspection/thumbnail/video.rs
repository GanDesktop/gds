use std::{
    fs::{self, File},
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use super::{EDGE, Error};

const TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(25);
const ERROR_BYTES: u64 = 4 * 1024;
const MAX_FRAME_BYTES: u64 = (EDGE as u64) * (EDGE as u64) * 4 + 64 * 1024;

/// # Errors
/// Reports unavailable FFmpeg, extraction failures, timeouts and filesystem errors.
pub(super) fn load(path: &Path) -> Result<::image::RgbaImage, Error> {
    let directory = tempfile::tempdir()?;
    let frame = directory.path().join("frame.png");
    let errors = directory.path().join("ffmpeg.log");
    let stderr = File::create(&errors)?;
    let child = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-v",
            "error",
            "-threads",
            "1",
            "-filter_threads",
            "1",
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(path)
        .args(["-map", "0:v:0", "-frames:v", "1", "-vf"])
        .arg(format!(
            "scale={EDGE}:{EDGE}:force_original_aspect_ratio=decrease"
        ))
        .args(["-threads", "1", "-f", "image2", "-update", "1", "-y"])
        .arg(&frame)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::VideoUnavailable
            } else {
                Error::Io(error)
            }
        })?;
    let mut child = Running(child);
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        if started.elapsed() >= TIMEOUT {
            return Err(Error::VideoTimeout);
        }
        std::thread::sleep(POLL_INTERVAL);
    };
    if !status.success() || !frame.is_file() {
        let mut message = String::new();
        File::open(errors)?
            .take(ERROR_BYTES)
            .read_to_string(&mut message)?;
        return Err(Error::Video(format!("{status}: {}", message.trim())));
    }
    if fs::metadata(&frame)?.len() > MAX_FRAME_BYTES {
        return Err(Error::TooLarge);
    }
    Ok(
        ::image::load_from_memory_with_format(&fs::read(frame)?, ::image::ImageFormat::Png)?
            .to_rgba8(),
    )
}

struct Running(std::process::Child);

impl Drop for Running {
    fn drop(&mut self) {
        match self.0.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "cannot inspect thumbnail worker during cleanup"),
        }
        if let Err(error) = self.0.kill() {
            tracing::warn!(%error, "cannot stop thumbnail worker");
        }
        if let Err(error) = self.0.wait() {
            tracing::warn!(%error, "cannot reap thumbnail worker");
        }
    }
}
