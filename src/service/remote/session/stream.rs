use super::super::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct Stream {
    backend: Backend,
    offset: u64,
}
enum Backend {
    Sftp(russh_sftp::client::fs::File),
    Smb(smb::File),
}

impl Stream {
    pub(super) fn sftp(file: russh_sftp::client::fs::File) -> Self {
        Self {
            backend: Backend::Sftp(file),
            offset: 0,
        }
    }
    pub(super) fn smb(file: smb::File) -> Self {
        Self {
            backend: Backend::Smb(file),
            offset: 0,
        }
    }

    /// # Errors
    /// Reports a failed remote read.
    pub async fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Error> {
        let count = match &mut self.backend {
            Backend::Sftp(file) => file.read(buffer).await?,
            Backend::Smb(file) => file.read_block(buffer, self.offset, None, false).await?,
        };
        self.offset += count as u64;
        Ok(count)
    }

    /// # Errors
    /// Reports failed or zero-length writes; callers must also check close.
    pub async fn write_all(&mut self, mut buffer: &[u8]) -> Result<(), Error> {
        while !buffer.is_empty() {
            let count = match &mut self.backend {
                Backend::Sftp(file) => file.write(buffer).await?,
                Backend::Smb(file) => file.write_block(buffer, self.offset, None).await?,
            };
            if count == 0 {
                return Err(std::io::Error::from(std::io::ErrorKind::WriteZero).into());
            }
            self.offset += count as u64;
            buffer = &buffer[count..];
        }
        Ok(())
    }

    /// # Errors
    /// Reports a failed remote close or final write acknowledgement.
    pub async fn close(mut self) -> Result<(), Error> {
        match &mut self.backend {
            Backend::Sftp(file) => file.shutdown().await.map_err(Error::from),
            Backend::Smb(file) => file
                .close()
                .await
                .map_err(|e| Error::protocol("SMB close", e)),
        }
    }
}
