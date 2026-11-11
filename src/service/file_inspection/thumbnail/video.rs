use std::{
    fs::{self, File},
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

use super::{EDGE, Error};

const TIMEOUT: Duration = Duration::from_secs(10);
const ERROR_BYTES: u64 = 4 * 1024;
const MAX_FRAME_BYTES: u64 = (EDGE as u64) * (EDGE as u64) * 4 + 64 * 1024;

/// # Errors
/// Reports unavailable FFmpeg, extraction failures, timeouts and filesystem errors.
pub(super) fn load(
    path: &Path,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<::image::RgbaImage, Error> {
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
    let mut child = crate::service::task_manager::process::ChildGuard::new(child);
    let status = child.wait(cancel, TIMEOUT).map_err(|error| {
        if error.kind() == std::io::ErrorKind::TimedOut {
            Error::VideoTimeout
        } else {
            Error::Io(error)
        }
    })?;
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
