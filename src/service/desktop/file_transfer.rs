//! XDG Document Portal file-transfer helper (`org.freedesktop.portal.FileTransfer`).
//!
//! Enables clipboard and drag-and-drop file exchange across sandbox boundaries (Flatpak, Snap,
//! etc.) using the `application/vnd.portal.filetransfer` MIME type. Regular files and directories
//! are exported together via `O_PATH | O_CLOEXEC` descriptors batched to respect D-Bus FD limits,
//! rolling back atomically via `StopTransfer` on any failure so partial selections are never
//! silently exposed.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// Standard MIME type carrying an opaque `org.freedesktop.portal.FileTransfer` session key.
pub const MIME_TYPE: &str = "application/vnd.portal.filetransfer";

/// Maximum number of file descriptors sent in a single `AddFiles` D-Bus call.
pub const ADD_FILES_BATCH_SIZE: usize = 16;

/// Bounded wait used when lazily exporting files from a background clipboard/drag writer thread.
pub const DEFAULT_EXPORT_TIMEOUT: Duration = Duration::from_secs(4);

/// Maximum byte length accepted for a decoded transfer key payload.
const MAX_KEY_BYTES: usize = 4096;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no file paths were provided for portal file transfer")]
    EmptyPaths,
    #[error("portal file transfer path must be absolute: {0}")]
    InvalidPath(PathBuf),
    #[error("portal file transfer key is empty or malformed")]
    InvalidKey,
    #[error("cannot open path {path} for portal file transfer: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    #[error("cannot connect to session D-Bus for portal file transfer: {0}")]
    Connection(#[source] zbus::Error),
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    #[error("portal file transfer request failed: {0}")]
    Portal(#[source] ashpd::Error),
    #[error("portal file transfer timed out")]
    Timeout,
    #[error("portal file transfer is not supported on this platform")]
    UnsupportedPlatform,
}

/// Decodes a raw `application/vnd.portal.filetransfer` MIME payload into a validated session key.
///
/// Tolerates optional trailing NUL bytes or ASCII whitespace emitted by some toolkits while
/// rejecting empty, oversized, non-UTF-8, or control-character-bearing payloads.
#[must_use]
pub fn decode_transfer_key(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() || bytes.len() > MAX_KEY_BYTES {
        return None;
    }
    let without_nuls = bytes.trim_ascii_end();
    let trimmed_bytes = strip_trailing_nuls(without_nuls);
    let text = std::str::from_utf8(trimmed_bytes).ok()?;
    let key = text.trim();
    if key.is_empty() || key.chars().any(char::is_control) {
        return None;
    }
    Some(key.to_owned())
}

fn strip_trailing_nuls(mut bytes: &[u8]) -> &[u8] {
    while let Some(rest) = bytes.strip_suffix(b"\0") {
        bytes = rest;
    }
    bytes
}

/// Returns `true` when the current process runs inside a Flatpak or Snap filesystem sandbox.
#[must_use]
pub fn is_sandboxed() -> bool {
    #[cfg(target_os = "linux")]
    {
        Path::new("/.flatpak-info").exists() || std::env::var_os("SNAP").is_some()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod unix_portal {
    use std::{collections::HashSet, os::fd::OwnedFd, path::PathBuf, time::Duration};

    use ashpd::documents::file_transfer::{
        AddFilesOptions, FileTransfer, RetrieveFilesOptions, StartTransferOptions,
    };
    use rustix::fs::{Mode, OFlags};

    use super::{ADD_FILES_BATCH_SIZE, Error};

    pub(super) fn open_path_fds(paths: &[PathBuf]) -> Result<Vec<OwnedFd>, Error> {
        if paths.is_empty() {
            return Err(Error::EmptyPaths);
        }
        #[cfg(target_os = "linux")]
        let flags = OFlags::PATH | OFlags::CLOEXEC;
        #[cfg(not(target_os = "linux"))]
        let flags = OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC;

        let mut seen = HashSet::with_capacity(paths.len());
        let mut fds = Vec::with_capacity(paths.len());
        for path in paths {
            if !path.is_absolute() {
                return Err(Error::InvalidPath(path.clone()));
            }
            if !seen.insert(path.as_path()) {
                continue;
            }
            let fd = rustix::fs::open(path, flags, Mode::empty()).map_err(|errno| Error::Open {
                path: path.clone(),
                source: std::io::Error::from(errno),
            })?;
            fds.push(fd);
        }
        if fds.is_empty() {
            return Err(Error::EmptyPaths);
        }
        Ok(fds)
    }

    pub(super) async fn export_files_with_proxy(
        proxy: &FileTransfer,
        paths: &[PathBuf],
        writable: bool,
        auto_stop: bool,
    ) -> Result<String, Error> {
        let fds = open_path_fds(paths)?;
        let key = proxy
            .start_transfer(
                StartTransferOptions::default()
                    .set_writeable(writable)
                    .set_auto_stop(auto_stop),
            )
            .await
            .map_err(Error::Portal)?;
        for batch in fds.chunks(ADD_FILES_BATCH_SIZE) {
            if let Err(error) = proxy
                .add_files(&key, batch, AddFilesOptions::default())
                .await
            {
                if let Err(stop_err) = proxy.stop_transfer(&key).await {
                    tracing::debug!(
                        %stop_err,
                        key = %key,
                        "failed to roll back portal file transfer after AddFiles error"
                    );
                }
                return Err(Error::Portal(error));
            }
        }
        Ok(key)
    }

    pub(super) async fn retrieve_files_with_proxy(
        proxy: &FileTransfer,
        key: &str,
    ) -> Result<Vec<PathBuf>, Error> {
        let trimmed = key.trim();
        if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
            return Err(Error::InvalidKey);
        }
        let raw_paths = proxy
            .retrieve_files(trimmed, RetrieveFilesOptions::default())
            .await
            .map_err(Error::Portal)?;
        let mut resolved = Vec::with_capacity(raw_paths.len());
        let mut seen = HashSet::with_capacity(raw_paths.len());
        for raw in raw_paths {
            if raw.is_empty() {
                continue;
            }
            let path = PathBuf::from(raw);
            if !path.is_absolute() {
                continue;
            }
            if seen.insert(path.clone()) {
                resolved.push(path);
            }
        }
        if resolved.is_empty() {
            return Err(Error::EmptyPaths);
        }
        Ok(resolved)
    }

    /// Exports local files and directories through `org.freedesktop.portal.FileTransfer`.
    ///
    /// # Errors
    /// Reports invalid/unopenable paths, D-Bus connection failures, or portal rejections.
    pub async fn export_files(
        paths: &[PathBuf],
        writable: bool,
        auto_stop: bool,
    ) -> Result<String, Error> {
        let connection = zbus::Connection::session()
            .await
            .map_err(Error::Connection)?;
        let proxy = FileTransfer::with_connection(connection)
            .await
            .map_err(Error::Portal)?;
        export_files_with_proxy(&proxy, paths, writable, auto_stop).await
    }

    /// Retrieves exported file paths for a `org.freedesktop.portal.FileTransfer` session `key`.
    ///
    /// # Errors
    /// Reports malformed keys, D-Bus connection failures, portal rejections, or empty results.
    pub async fn retrieve_files(key: &str) -> Result<Vec<PathBuf>, Error> {
        let connection = zbus::Connection::session()
            .await
            .map_err(Error::Connection)?;
        let proxy = FileTransfer::with_connection(connection)
            .await
            .map_err(Error::Portal)?;
        retrieve_files_with_proxy(&proxy, key).await
    }

    /// Blocking bridge for lazily exporting files from a background clipboard or drag-source
    /// writer thread. Returns `None` on failure or timeout so the peer pipe closes cleanly.
    #[must_use]
    pub fn export_files_blocking(
        runtime: &tokio::runtime::Handle,
        paths: &[PathBuf],
        writable: bool,
        timeout: Duration,
    ) -> Option<Vec<u8>> {
        let result = runtime.block_on(async {
            match tokio::time::timeout(timeout, export_files(paths, writable, true)).await {
                Ok(inner) => inner,
                Err(_) => Err(Error::Timeout),
            }
        });
        match result {
            Ok(key) => Some(key.into_bytes()),
            Err(error) => {
                tracing::debug!(%error, count = paths.len(), "lazy portal file transfer export declined");
                None
            }
        }
    }

    /// Returns `true` when every local `path` is visible on this host filesystem (without
    /// following symlinks).
    #[must_use]
    pub fn all_paths_accessible(paths: &[PathBuf]) -> bool {
        !paths.is_empty()
            && paths
                .iter()
                .all(|path| std::fs::symlink_metadata(path).is_ok())
    }

    #[cfg(test)]
    pub(super) fn is_fd_valid(path: &std::path::Path) -> bool {
        open_path_fds(&[path.to_path_buf()]).is_ok()
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub use unix_portal::{all_paths_accessible, export_files, export_files_blocking, retrieve_files};

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
/// Exports local files through `org.freedesktop.portal.FileTransfer` (unsupported on this OS).
///
/// # Errors
/// Always returns [`Error::UnsupportedPlatform`].
pub async fn export_files(
    _paths: &[PathBuf],
    _writable: bool,
    _auto_stop: bool,
) -> Result<String, Error> {
    Err(Error::UnsupportedPlatform)
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
/// Retrieves exported file paths for a portal transfer key (unsupported on this OS).
///
/// # Errors
/// Always returns [`Error::UnsupportedPlatform`].
pub async fn retrieve_files(_key: &str) -> Result<Vec<PathBuf>, Error> {
    Err(Error::UnsupportedPlatform)
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
/// Blocking bridge for lazy portal export (always returns `None` on non-Linux/FreeBSD).
#[must_use]
pub fn export_files_blocking(
    _runtime: &tokio::runtime::Handle,
    _paths: &[PathBuf],
    _writable: bool,
    _timeout: Duration,
) -> Option<Vec<u8>> {
    None
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
/// Returns `true` when every local `path` is visible on this host filesystem.
#[must_use]
pub fn all_paths_accessible(paths: &[PathBuf]) -> bool {
    !paths.is_empty()
        && paths
            .iter()
            .all(|path| std::fs::symlink_metadata(path).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_transfer_key_trims_trailing_nuls_and_rejects_malformed_inputs() {
        assert_eq!(decode_transfer_key(b"123456"), Some("123456".to_owned()));
        assert_eq!(
            decode_transfer_key(b"transfer-key\0"),
            Some("transfer-key".to_owned())
        );
        assert_eq!(
            decode_transfer_key(b"  transfer-key\0\0\n"),
            Some("transfer-key".to_owned())
        );
        assert_eq!(decode_transfer_key(b""), None);
        assert_eq!(decode_transfer_key(b"\0\0"), None);
        assert_eq!(decode_transfer_key(b"bad\nkey"), None);
        assert_eq!(decode_transfer_key(b"bad\0mid"), None);
        assert_eq!(decode_transfer_key(&[0xFF, 0xFE]), None);
        assert_eq!(decode_transfer_key(&vec![b'a'; MAX_KEY_BYTES + 1]), None);
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    #[test]
    fn open_path_fds_handles_files_directories_symlinks_and_rejects_missing_atomically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("doc.txt");
        let subdir = dir.path().join("folder");
        let symlink = dir.path().join("doc-link.txt");
        let dangling = dir.path().join("dangling-link");
        std::fs::write(&file, b"hello").expect("write file");
        std::fs::create_dir(&subdir).expect("create subdir");
        std::os::unix::fs::symlink(&file, &symlink).expect("create symlink");
        std::os::unix::fs::symlink(dir.path().join("missing"), &dangling)
            .expect("create dangling symlink");

        let fds = unix_portal::open_path_fds(&[
            file.clone(),
            subdir.clone(),
            symlink.clone(),
            file.clone(),
        ])
        .expect("files, dirs, and valid symlinks open with O_PATH");
        assert_eq!(fds.len(), 3, "duplicates are deduplicated");
        assert!(unix_portal::is_fd_valid(&subdir));

        assert!(matches!(
            unix_portal::open_path_fds(&[PathBuf::from("relative/path.txt")]),
            Err(Error::InvalidPath(_))
        ));
        assert!(matches!(
            unix_portal::open_path_fds(&[file, dangling]),
            Err(Error::Open { .. })
        ));
        assert!(matches!(
            unix_portal::open_path_fds(&[]),
            Err(Error::EmptyPaths)
        ));
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    #[tokio::test]
    async fn mock_file_transfer_portal_batches_files_and_dirs_and_rolls_back_on_error() {
        use std::collections::HashMap;
        use std::os::fd::AsRawFd;
        use std::sync::{Arc, Mutex};
        use tokio::net::UnixStream;
        use zbus::{
            conn::Builder,
            interface,
            zvariant::{OwnedFd as ZbusOwnedFd, OwnedValue},
        };

        #[derive(Default)]
        struct PortalState {
            next_id: u32,
            start_options: Vec<(Option<bool>, Option<bool>)>,
            batch_sizes: Vec<usize>,
            stopped: Vec<String>,
            fail_on_batch: Option<usize>,
            stored_paths: HashMap<String, Vec<String>>,
        }

        struct MockFileTransfer {
            state: Arc<Mutex<PortalState>>,
        }

        #[interface(name = "org.freedesktop.portal.FileTransfer")]
        impl MockFileTransfer {
            fn start_transfer(&self, options: HashMap<String, OwnedValue>) -> String {
                let mut state = self.state.lock().expect("state lock");
                state.next_id += 1;
                let writeable = options
                    .get("writeable")
                    .and_then(|v| bool::try_from(v).ok());
                let autostop = options.get("autostop").and_then(|v| bool::try_from(v).ok());
                state.start_options.push((writeable, autostop));
                format!("key-{}", state.next_id)
            }

            fn add_files(
                &self,
                key: String,
                fds: Vec<ZbusOwnedFd>,
                _options: HashMap<String, OwnedValue>,
            ) -> zbus::fdo::Result<()> {
                let mut state = self.state.lock().expect("state lock");
                let batch_index = state.batch_sizes.len() + 1;
                if state.fail_on_batch == Some(batch_index) {
                    return Err(zbus::fdo::Error::Failed("simulated batch failure".into()));
                }
                state.batch_sizes.push(fds.len());
                let entry = state.stored_paths.entry(key).or_default();
                for fd in fds {
                    // Verify every received descriptor is a valid open FD.
                    let raw = fd.as_raw_fd();
                    assert!(raw >= 0);
                    let link = format!("/proc/self/fd/{raw}");
                    let path = std::fs::read_link(&link)
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|_| format!("/exported/{}", entry.len()));
                    entry.push(path);
                }
                Ok(())
            }

            fn retrieve_files(
                &self,
                key: String,
                _options: HashMap<String, OwnedValue>,
            ) -> zbus::fdo::Result<Vec<String>> {
                let state = self.state.lock().expect("state lock");
                state
                    .stored_paths
                    .get(&key)
                    .cloned()
                    .ok_or_else(|| zbus::fdo::Error::Failed("unknown transfer key".into()))
            }

            fn stop_transfer(&self, key: String) {
                let mut state = self.state.lock().expect("state lock");
                state.stopped.push(key.clone());
                state.stored_paths.remove(&key);
            }

            #[zbus(property)]
            fn version(&self) -> u32 {
                1
            }
        }

        let (server_sock, client_sock) = UnixStream::pair().expect("unix socket pair");
        let state = Arc::new(Mutex::new(PortalState::default()));
        let guid = zbus::Guid::generate();

        let server_fut = Builder::unix_stream(server_sock)
            .p2p()
            .server(guid)
            .expect("server guid")
            .serve_at(
                "/org/freedesktop/portal/documents",
                MockFileTransfer {
                    state: Arc::clone(&state),
                },
            )
            .expect("serve mock interface")
            .build();
        let client_fut = Builder::unix_stream(client_sock).p2p().build();
        let (_server_conn, client_conn) =
            tokio::try_join!(server_fut, client_fut).expect("p2p dbus connections established");

        let proxy = ashpd::documents::file_transfer::FileTransfer::with_connection(client_conn)
            .await
            .expect("proxy connected");

        // 1. Export 18 files + 2 directories (20 total -> 16 + 4 batches).
        let temp = tempfile::tempdir().expect("tempdir");
        let mut paths = Vec::new();
        for idx in 0..18 {
            let p = temp.path().join(format!("item-{idx:02}.txt"));
            std::fs::write(&p, b"data").expect("write item");
            paths.push(p);
        }
        for idx in 0..2 {
            let d = temp.path().join(format!("dir-{idx:02}"));
            std::fs::create_dir(&d).expect("create dir");
            paths.push(d);
        }

        let key = unix_portal::export_files_with_proxy(&proxy, &paths, true, true)
            .await
            .expect("20 entries exported in two batches");
        assert_eq!(key, "key-1");

        let retrieved = unix_portal::retrieve_files_with_proxy(&proxy, &key)
            .await
            .expect("retrieve succeeds");
        assert_eq!(retrieved.len(), 20);
        assert_eq!(retrieved, paths);
        {
            let guard = state.lock().expect("state lock");
            assert_eq!(guard.start_options, vec![(Some(true), Some(true))]);
            assert_eq!(guard.batch_sizes, vec![16, 4]);
            assert!(guard.stopped.is_empty());
        }

        // 2. Simulate batch-2 failure -> must atomically call StopTransfer(key-2).
        {
            let mut guard = state.lock().expect("state lock");
            guard.batch_sizes.clear();
            guard.fail_on_batch = Some(2);
        }
        let err = unix_portal::export_files_with_proxy(&proxy, &paths, false, true)
            .await
            .expect_err("second batch fails");
        assert!(matches!(err, Error::Portal(_)));
        {
            let guard = state.lock().expect("state lock");
            assert_eq!(guard.batch_sizes, vec![16]);
            assert_eq!(guard.stopped, vec!["key-2".to_owned()]);
        }
    }
}
