use super::{Clipboard, Error};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use std::{
    io::{self, Read},
    os::fd::AsFd,
    time::Instant,
};
const BUFFER_SIZE: usize = 8192;

impl Clipboard {
    /// Read a clipboard pipe until EOF with one deadline and a caller-selected size limit.
    /// Call from a background executor.
    ///
    /// # Errors
    /// Reports a stalled source, oversized payload or an I/O error. Interrupted calls retry
    /// within the original deadline.
    pub(super) fn read_pipe(
        &self,
        mut pipe: impl Read + AsFd,
        deadline: Instant,
    ) -> Result<Vec<u8>, Error> {
        let mut contents = Vec::new();
        let mut buffer = [0u8; BUFFER_SIZE];
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or(Error::Timeout)?;
            let timeout = Timespec {
                tv_sec: remaining.as_secs() as _,
                tv_nsec: remaining.subsec_nanos() as _,
            };
            let mut fds = [PollFd::new(&pipe, PollFlags::IN)];
            match poll(&mut fds, Some(&timeout)) {
                Ok(0) => return Err(Error::Timeout),
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(io::Error::from(error).into()),
            }
            let count = match pipe.read(&mut buffer) {
                Ok(count) => count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            };
            if count == 0 {
                return Ok(contents);
            }
            if count > self.limit.saturating_sub(contents.len()) {
                return Err(Error::TooLarge);
            }
            contents.extend_from_slice(&buffer[..count]);
        }
    }
}
