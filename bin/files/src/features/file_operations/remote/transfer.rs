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

pub(crate) const DEFAULT_BUFFER_SIZE: usize = 128 * 1024;

#[derive(Clone, Copy, Debug)]
pub(crate) struct CopyOptions {
    pub buffer_size: usize,
}

impl Default for CopyOptions {
    fn default() -> Self {
        Self {
            buffer_size: DEFAULT_BUFFER_SIZE,
        }
    }
}

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
    pub undo: Vec<super::super::history::Step>,
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
    options: CopyOptions,
    cancel: CancellationToken,
    updates: watch::Sender<Progress>,
) -> Report {
    let mut work = Work {
        tasks,
        cancel,
        updates,
        progress: Progress::default(),
        buffer: vec![0; options.buffer_size.max(4096)],
    };
    let mut report = Report::default();
    for source in sources {
        if work.cancel.is_cancelled() {
            report.cancelled = true;
            break;
        }
        match work.copy_root(&source, &destination).await {
            Ok(undo) => {
                report.completed.push(source);
                report.undo.extend(undo);
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

    /// Create an exclusive local staging directory on the blocking pool, owned by a
    /// `TempDir` guard so cancellation, future drops, or rejected commit workers still clean up.
    ///
    /// # Errors
    /// Reports task scheduling or directory creation failures.
    async fn create_local_staging(&self, directory: &Path) -> Result<tempfile::TempDir, Error> {
        let directory = directory.to_path_buf();
        self.tasks
            .spawn_blocking_named("Create download staging directory", move || {
                tempfile::Builder::new()
                    .prefix(".gds-download-")
                    .tempdir_in(&directory)
                    .map_err(Error::Io)
            })
            .await?
    }

    /// Explicitly close a local staging directory off the async runtime, surfacing cleanup errors
    /// alongside `operation`. If the task scope rejects the worker during shutdown, dropping the
    /// closure still triggers `TempDir::drop`.
    async fn close_local_staging(&self, staging_dir: tempfile::TempDir, operation: Error) -> Error {
        let path = staging_dir.path().to_path_buf();
        match self
            .tasks
            .spawn_blocking_named("Clean up download staging directory", move || {
                staging_dir.close()
            })
            .await
        {
            Ok(Ok(())) | Err(_) => operation,
            Ok(Err(cleanup)) => Error::LocalStagingCleanup {
                path,
                operation: Box::new(operation),
                cleanup,
            },
        }
    }

    /// Commit a staged local payload into `directory` and explicitly close `staging_dir`.
    ///
    /// # Errors
    /// Reports transfer, commit worker scheduling, or staging cleanup failures.
    async fn commit_local_staging(
        &self,
        staging_dir: tempfile::TempDir,
        staged_path: PathBuf,
        directory: &Path,
    ) -> Result<Vec<super::super::history::Step>, Error> {
        let batch = match super::super::transfer::Batch::new(super::super::transfer::Request {
            paths: vec![staged_path],
            destination: directory.to_path_buf(),
            mode: super::super::transfer::Mode::Move,
        }) {
            Ok(batch) => batch,
            Err(error) => {
                return Err(self
                    .close_local_staging(staging_dir, Error::from(error))
                    .await);
            }
        };
        self.tasks
            .spawn_blocking_named(
                "Commit downloaded files",
                move || -> Result<Vec<super::super::history::Step>, Error> {
                    let result = batch.execute();
                    let mut undo = Vec::new();
                    let mut transfer_error = None;
                    for item in result.items {
                        match item.result {
                            Ok(()) => {
                                if let Some(expected) = item.committed_snapshot {
                                    undo.push(super::super::history::Step::trash(
                                        item.destination,
                                        expected,
                                    ));
                                }
                            }
                            Err(error) => {
                                transfer_error = Some(Error::Transfer(error));
                                break;
                            }
                        }
                    }
                    let staging_path = staging_dir.path().to_path_buf();
                    let cleanup = staging_dir.close();
                    match (transfer_error, cleanup) {
                        (None, Ok(())) => Ok(undo),
                        (None, Err(cleanup)) => Err(Error::Io(cleanup)),
                        (Some(operation), Ok(())) => Err(operation),
                        (Some(operation), Err(cleanup)) => Err(Error::LocalStagingCleanup {
                            path: staging_path,
                            operation: Box::new(operation),
                            cleanup,
                        }),
                    }
                },
            )
            .await?
    }

    /// # Errors
    /// Rejects self-nested copies and reports local or remote transfer failures.
    async fn copy_root(
        &mut self,
        source: &Resource,
        destination: &Resource,
    ) -> Result<Vec<super::super::history::Step>, Error> {
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
            Resource::Remote { address, session } => self
                .upload_to_remote(source, &target, address, session)
                .await
                .map(|()| Vec::new()),
        }
    }

    /// # Errors
    /// Reports staging creation, download, atomic commit or cleanup failures.
    async fn download_to_local(
        &mut self,
        source: &Resource,
        directory: &Path,
        name: &str,
    ) -> Result<Vec<super::super::history::Step>, Error> {
        if let Resource::Local(source) = source
            && tokio::fs::canonicalize(directory)
                .await?
                .starts_with(tokio::fs::canonicalize(source).await?)
        {
            return Err(Error::Unsupported("copying a directory into itself"));
        }
        let staging_dir = self.create_local_staging(directory).await?;
        let staged_path = staging_dir.path().join(name);
        let staged = Resource::Local(staged_path.clone());
        let copy_result = match self.copy_tree(source, &staged, Reservation::New).await {
            Ok(()) if self.cancel.is_cancelled() => Err(Error::Cancelled),
            other => other,
        };
        if let Err(operation) = copy_result {
            return Err(self.close_local_staging(staging_dir, operation).await);
        }
        self.commit_local_staging(staging_dir, staged_path, directory)
            .await
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

#[cfg(test)]
mod tests {
    use super::*;
    use gds::service::task_manager::{Scope, Supervisor};

    fn staging_entries(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .expect("read destination directory")
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                path.file_name()?
                    .to_string_lossy()
                    .starts_with(".gds-download-")
                    .then_some(path)
            })
            .collect()
    }

    #[test]
    fn nested_download_commits_and_cleans_staging_directory() {
        let supervisor = Supervisor::new(false).expect("test runtime");
        let scope = supervisor.scope();
        let src_root = tempfile::tempdir().expect("source tempdir");
        let dst_root = tempfile::tempdir().expect("destination tempdir");

        let nested = src_root.path().join("bundle/sub");
        std::fs::create_dir_all(&nested).expect("create nested dir");
        std::fs::write(nested.join("notes.txt"), b"hello nested").expect("write nested file");

        let (tx, _rx) = watch::channel(Progress::default());
        let report = scope
            .block_on(copy(
                scope.clone(),
                vec![Resource::Local(src_root.path().join("bundle"))],
                Resource::Local(dst_root.path().to_path_buf()),
                CopyOptions::default(),
                CancellationToken::new(),
                tx,
            ))
            .expect("run download");

        assert!(report.failures.is_empty());
        assert!(!report.cancelled);
        assert_eq!(report.completed.len(), 1);
        assert_eq!(report.undo.len(), 1);
        assert_eq!(
            std::fs::read(dst_root.path().join("bundle/sub/notes.txt")).expect("read committed"),
            b"hello nested"
        );
        assert!(staging_entries(dst_root.path()).is_empty());
    }

    #[test]
    fn destination_collision_and_cancellation_leave_no_staging_directories() {
        let supervisor = Supervisor::new(false).expect("test runtime");
        let scope = supervisor.scope();
        let src_root = tempfile::tempdir().expect("source tempdir");
        let dst_root = tempfile::tempdir().expect("destination tempdir");

        let src_file = src_root.path().join("report.txt");
        let dst_file = dst_root.path().join("report.txt");
        std::fs::write(&src_file, b"incoming").expect("write source");
        std::fs::write(&dst_file, b"existing").expect("write existing destination");

        let (tx, _rx) = watch::channel(Progress::default());
        let collision_report = scope
            .block_on(copy(
                scope.clone(),
                vec![Resource::Local(src_file.clone())],
                Resource::Local(dst_root.path().to_path_buf()),
                CopyOptions::default(),
                CancellationToken::new(),
                tx,
            ))
            .expect("run colliding download");
        assert_eq!(collision_report.failures.len(), 1);
        assert_eq!(
            std::fs::read(&dst_file).expect("read existing"),
            b"existing"
        );
        assert!(staging_entries(dst_root.path()).is_empty());

        // Pre-cancelled download must abort before commit and clean up its staging directory.
        let cancel = CancellationToken::new();
        cancel.cancel();
        let (tx, _rx) = watch::channel(Progress::default());
        let mut work = Work {
            tasks: scope.clone(),
            cancel,
            updates: tx,
            progress: Progress::default(),
            buffer: vec![0; 4096],
        };
        let err = scope
            .block_on(work.download_to_local(
                &Resource::Local(src_file),
                dst_root.path(),
                "fresh.txt",
            ))
            .expect("drive cancelled download")
            .expect_err("cancelled download must fail");
        assert!(matches!(err, Error::Cancelled));
        assert!(!dst_root.path().join("fresh.txt").exists());
        assert!(staging_entries(dst_root.path()).is_empty());
    }

    #[test]
    fn rejected_commit_worker_drops_tempdir_and_cleans_staged_payload() {
        let supervisor = Supervisor::new(false).expect("test runtime");
        let active_scope = supervisor.scope();
        let dst_root = tempfile::tempdir().expect("destination tempdir");

        let staging = tempfile::Builder::new()
            .prefix(".gds-download-")
            .tempdir_in(dst_root.path())
            .expect("create staging tempdir");
        let staged_path = staging.path().join("payload.bin");
        std::fs::write(&staged_path, b"staged bytes").expect("write staged payload");
        let staging_path = staging.path().to_path_buf();
        assert!(staging_path.exists());

        // A default Scope has no executor, so `spawn_blocking_named` rejects the commit closure
        // without running it. Because `TempDir` is moved into the closure, dropping it removes
        // the staging directory automatically.
        let (tx, _rx) = watch::channel(Progress::default());
        let work = Work {
            tasks: Scope::default(),
            cancel: CancellationToken::new(),
            updates: tx,
            progress: Progress::default(),
            buffer: vec![0; 4096],
        };
        let err = active_scope
            .block_on(work.commit_local_staging(staging, staged_path, dst_root.path()))
            .expect("drive commit")
            .expect_err("rejected commit worker must report task error");
        assert!(matches!(err, Error::Task(_)));
        assert!(!staging_path.exists());
        assert!(staging_entries(dst_root.path()).is_empty());
    }
}
