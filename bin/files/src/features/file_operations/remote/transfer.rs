use super::{Error, Resource, resource::Stream};
use gds::service::remote::{Address, EntryKind, Session};
use std::{
    collections::hash_map::RandomState,
    future::Future,
    hash::{BuildHasher, Hasher},
    path::{Path, PathBuf},
    pin::Pin,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

const BUFFER_SIZE: usize = 128 * 1024;
const STAGING_ATTEMPTS: usize = 64;

static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Default)]
pub(crate) struct Progress {
    pub bytes: u64,
    pub files: usize,
    pub current: String,
}

#[derive(Default)]
pub(crate) struct Report {
    pub completed: Vec<Resource>,
    pub failures: Vec<(String, String)>,
    pub cancelled: bool,
}

struct Work {
    tasks: gds::service::task_manager::Scope,
    cancel: CancellationToken,
    updates: watch::Sender<Progress>,
    progress: Progress,
    buffer: Vec<u8>,
}

enum Reservation {
    New,
    Directory,
    File(Stream),
}

/// Copy each root to an exclusive staging entry and publish without replacing targets.
pub(crate) async fn copy(
    tasks: gds::service::task_manager::Scope,
    sources: Vec<Resource>,
    destination: Resource,
    cancel: CancellationToken,
    updates: watch::Sender<Progress>,
) -> Report {
    let mut work = Work {
        tasks,
        cancel,
        updates,
        progress: Progress::default(),
        buffer: vec![0; BUFFER_SIZE],
    };
    let mut report = Report::default();
    for source in sources {
        if work.cancel.is_cancelled() {
            report.cancelled = true;
            break;
        }
        match work.copy_root(&source, &destination).await {
            Ok(()) => report.completed.push(source),
            Err(Error::Cancelled) => {
                report.cancelled = true;
                break;
            }
            Err(error) => report.failures.push((source.label(), error.to_string())),
        }
    }
    report
}

/// Remove trees without following links. Completed roots remain reported after cancellation.
pub(crate) async fn delete(
    sources: Vec<Resource>,
    cancel: CancellationToken,
    updates: watch::Sender<Progress>,
) -> Report {
    let mut report = Report::default();
    for source in sources {
        match source.remove_tree(&cancel).await {
            Ok(()) => {
                report.completed.push(source);
                updates.send_replace(Progress {
                    files: report.completed.len(),
                    ..Default::default()
                });
            }
            Err(Error::Cancelled) => {
                report.cancelled = true;
                break;
            }
            Err(error) => report.failures.push((source.label(), error.to_string())),
        }
    }
    report
}

impl Work {
    /// Generate a unique staging leaf name purely in memory without touching local disk.
    fn staging_name(prefix: &str) -> String {
        let seq = STAGING_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(seq);
        hasher.write_u128(nanos);
        let nonce = hasher.finish();
        format!("{prefix}{nanos:016x}-{seq:04x}-{nonce:016x}")
    }

    /// Create an exclusive local staging directory asynchronously without blocking Tokio workers.
    ///
    /// # Errors
    /// Reports directory creation failures.
    async fn create_local_staging(directory: &Path) -> Result<PathBuf, Error> {
        for _ in 0..STAGING_ATTEMPTS {
            let candidate = directory.join(Self::staging_name(".gds-download-"));
            match tokio::fs::create_dir(&candidate).await {
                Ok(()) => return Ok(candidate),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(Error::Io(error)),
            }
        }
        Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate unique download staging directory",
        )))
    }

    /// # Errors
    /// Rejects self-nested copies and reports local or remote transfer failures.
    async fn copy_root(&mut self, source: &Resource, destination: &Resource) -> Result<(), Error> {
        let name = source.name()?;
        let target = destination.child(&name)?;
        if let (
            Resource::Remote {
                address: source, ..
            },
            Resource::Remote {
                address: target, ..
            },
        ) = (source, &target)
            && source.connection_key() == target.connection_key()
            && (source.path() == target.path()
                || target
                    .path()
                    .starts_with(&format!("{}/", source.path().trim_end_matches('/'))))
        {
            return Err(Error::Unsupported("copying a directory into itself"));
        }
        match destination {
            Resource::Local(directory) => self.download_to_local(source, directory, &name).await,
            Resource::Remote { address, session } => {
                self.upload_to_remote(source, &target, address, session)
                    .await
            }
        }
    }

    /// # Errors
    /// Reports staging creation, download, atomic commit or cleanup failures.
    async fn download_to_local(
        &mut self,
        source: &Resource,
        directory: &Path,
        name: &str,
    ) -> Result<(), Error> {
        if let Resource::Local(source) = source
            && tokio::fs::canonicalize(directory)
                .await?
                .starts_with(tokio::fs::canonicalize(source).await?)
        {
            return Err(Error::Unsupported("copying a directory into itself"));
        }
        let staging_dir = Self::create_local_staging(directory).await?;
        let staged_path = staging_dir.join(name);
        let staged = Resource::Local(staged_path.clone());
        let copy_result = match self.copy_tree(source, &staged, Reservation::New).await {
            Ok(()) if self.cancel.is_cancelled() => Err(Error::Cancelled),
            other => other,
        };
        if let Err(operation) = copy_result {
            if let Err(cleanup) = tokio::fs::remove_dir_all(&staging_dir).await {
                return Err(Error::LocalStagingCleanup {
                    path: staging_dir,
                    operation: Box::new(operation),
                    cleanup,
                });
            }
            return Err(operation);
        }
        let batch = super::super::transfer::Batch::new(super::super::transfer::Request {
            paths: vec![staged_path],
            destination: directory.to_path_buf(),
            mode: super::super::transfer::Mode::Move,
        })?;
        self.tasks
            .spawn_blocking_named("Commit downloaded files", move || -> Result<(), Error> {
                let result = batch.execute();
                let mut transfer_error = None;
                for item in result.items {
                    if let Err(error) = item.result {
                        transfer_error = Some(Error::Transfer(error));
                        break;
                    }
                }
                let cleanup = std::fs::remove_dir_all(&staging_dir);
                match (transfer_error, cleanup) {
                    (None, Ok(())) => Ok(()),
                    (None, Err(cleanup)) => Err(Error::Io(cleanup)),
                    (Some(operation), Ok(())) => Err(operation),
                    (Some(operation), Err(cleanup)) => Err(Error::LocalStagingCleanup {
                        path: staging_dir,
                        operation: Box::new(operation),
                        cleanup,
                    }),
                }
            })
            .await?
    }

    /// # Errors
    /// Reports remote staging, upload, atomic rename or cleanup failures.
    async fn upload_to_remote(
        &mut self,
        source: &Resource,
        target: &Resource,
        address: &Address,
        session: &Session,
    ) -> Result<(), Error> {
        let stage = address.child(&Self::staging_name(".gds-upload-"))?;
        let staged = Resource::Remote {
            address: stage.clone(),
            session: session.clone(),
        };
        let kind = source.kind().await?;
        if !matches!(kind, EntryKind::File | EntryKind::Directory) {
            return Err(Error::Unsupported("symbolic links and special files"));
        }
        let reserved = if kind == EntryKind::Directory {
            staged.mkdir().await?;
            Reservation::Directory
        } else {
            Reservation::File(staged.stream(true).await?)
        };
        let result = self.copy_tree(source, &staged, reserved).await;
        let result = match result {
            Ok(()) if self.cancel.is_cancelled() => Err(Error::Cancelled),
            Ok(()) => {
                let Resource::Remote {
                    address: target, ..
                } = target
                else {
                    unreachable!()
                };
                session.rename(&stage, target).await.map_err(Error::from)
            }
            Err(error) => Err(error),
        };
        if let Err(operation) = result {
            let cleanup = staged.remove_tree(&CancellationToken::new()).await;
            return match cleanup {
                Ok(()) => Err(operation),
                Err(cleanup) => Err(Error::StagingCleanup {
                    stage,
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                }),
            };
        }
        Ok(())
    }

    /// # Errors
    /// Propagates stream I/O, directory traversal and cancellation errors.
    fn copy_tree<'a>(
        &'a mut self,
        source: &'a Resource,
        target: &'a Resource,
        reserved: Reservation,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
        Box::pin(async move {
            self.progress.current = source.label();
            self.updates.send_replace(self.progress.clone());
            let kind = match &reserved {
                Reservation::New => source.kind().await?,
                Reservation::Directory => EntryKind::Directory,
                Reservation::File(_) => EntryKind::File,
            };
            match kind {
                EntryKind::Directory => {
                    if self.cancel.is_cancelled() {
                        return Err(Error::Cancelled);
                    }
                    if !matches!(reserved, Reservation::Directory) {
                        target.mkdir().await?;
                    }
                    for child in source.children().await? {
                        let target = target.child(&child.name()?)?;
                        self.copy_tree(&child, &target, Reservation::New).await?;
                    }
                }
                EntryKind::File => {
                    let mut output = match reserved {
                        Reservation::File(file) => file,
                        _ => target.stream(true).await?,
                    };
                    let mut input = match source.stream(false).await {
                        Ok(file) => file,
                        Err(error) => {
                            let close = output.close().await;
                            return Err(error.combine_close(close));
                        }
                    };
                    let copied: Result<(), Error> = async {
                        loop {
                            let count = self
                                .cancel
                                .run_until_cancelled(input.read(&mut self.buffer))
                                .await
                                .ok_or(Error::Cancelled)??;
                            if count == 0 {
                                break;
                            }
                            self.cancel
                                .run_until_cancelled(output.write_all(&self.buffer[..count]))
                                .await
                                .ok_or(Error::Cancelled)??;
                            self.progress.bytes += count as u64;
                            self.updates.send_replace(self.progress.clone());
                        }
                        Ok(())
                    }
                    .await;
                    let read_closed = input.close().await;
                    let write_closed = output.close().await;
                    match copied {
                        Ok(()) => {
                            read_closed?;
                            write_closed?;
                        }
                        Err(error) => {
                            return Err(error
                                .combine_close(read_closed)
                                .combine_close(write_closed));
                        }
                    }
                    self.progress.files += 1;
                    self.updates.send_replace(self.progress.clone());
                }
                _ => return Err(Error::Unsupported("symbolic links and special files")),
            }
            Ok(())
        })
    }
}
