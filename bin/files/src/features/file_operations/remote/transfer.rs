use super::{Error, Resource, resource::Stream};
use gds::service::remote::EntryKind;
use std::{future::Future, pin::Pin};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

const BUFFER_SIZE: usize = 128 * 1024;

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
    cancel: CancellationToken,
    updates: watch::Sender<Progress>,
    progress: Progress,
}
enum Reservation {
    New,
    Directory,
    File(Stream),
}

/// Copy each root to an exclusive staging entry and publish without replacing targets.
pub(crate) async fn copy(
    sources: Vec<Resource>,
    destination: Resource,
    cancel: CancellationToken,
    updates: watch::Sender<Progress>,
) -> Report {
    let mut work = Work {
        cancel,
        updates,
        progress: Progress::default(),
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
        match remove_tree(&source, &cancel).await {
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
        {
            if source.connection_key() == target.connection_key()
                && (source.path() == target.path()
                    || target
                        .path()
                        .starts_with(&format!("{}/", source.path().trim_end_matches('/'))))
            {
                return Err(Error::Unsupported("copying a directory into itself"));
            }
        }
        match destination {
            Resource::Local(directory) => {
                if let Resource::Local(source) = source {
                    if tokio::fs::canonicalize(directory)
                        .await?
                        .starts_with(tokio::fs::canonicalize(source).await?)
                    {
                        return Err(Error::Unsupported("copying a directory into itself"));
                    }
                }
                let staging = tempfile::Builder::new()
                    .prefix(".gds-download-")
                    .tempdir_in(directory)?;
                let staged = Resource::Local(staging.path().join(&name));
                self.copy_tree(source, &staged, Reservation::New).await?;
                if self.cancel.is_cancelled() {
                    return Err(Error::Cancelled);
                }
                let Resource::Local(path) = staged else {
                    unreachable!()
                };
                let batch = super::super::transfer::Batch::new(super::super::transfer::Request {
                    paths: vec![path],
                    destination: directory.clone(),
                    mode: super::super::transfer::Mode::Move,
                })
                .map_err(|e| Error::Failed(e.to_string()))?;
                let result = tokio::task::spawn_blocking(move || batch.execute())
                    .await
                    .map_err(|e| Error::Failed(e.to_string()))?;
                if let Some(item) = result.items.into_iter().find(|item| item.result.is_err()) {
                    return Err(Error::Failed(
                        item.result.expect_err("failed transfer item").to_string(),
                    ));
                }
                Ok(())
            }
            Resource::Remote { address, session } => {
                let nonce = tempfile::Builder::new()
                    .prefix(".gds-upload-")
                    .rand_bytes(16)
                    .tempfile()?;
                let stage = address.child(
                    nonce
                        .path()
                        .file_name()
                        .expect("temporary file name")
                        .to_str()
                        .expect("ASCII staging name"),
                )?;
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
                        session.rename(&stage, &target).await.map_err(Error::from)
                    }
                    Err(error) => Err(error),
                };
                if let Err(error) = result {
                    let cleanup = remove_tree(&staged, &CancellationToken::new()).await;
                    return match cleanup {
                        Ok(()) => Err(error),
                        Err(cleanup) => Err(Error::Failed(format!(
                            "{error}; cannot remove staging {stage}: {cleanup}"
                        ))),
                    };
                }
                Ok(())
            }
        }
    }

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
                            return Err(combine(error, close));
                        }
                    };
                    let copied = async {
                        let mut buffer = vec![0; BUFFER_SIZE];
                        loop {
                            let count = tokio::select! { _ = self.cancel.cancelled() => return Err(Error::Cancelled), result = input.read(&mut buffer) => result? };
                            if count == 0 { break; }
                            tokio::select! { _ = self.cancel.cancelled() => return Err(Error::Cancelled), result = output.write_all(&buffer[..count]) => result? }
                            self.progress.bytes += count as u64; self.updates.send_replace(self.progress.clone());
                        }
                        Ok(())
                    }.await;
                    let read_closed = input.close().await;
                    let write_closed = output.close().await;
                    match copied {
                        Ok(()) => {
                            read_closed?;
                            write_closed?;
                        }
                        Err(error) => {
                            return Err(combine(combine(error, read_closed), write_closed));
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

fn combine(error: Error, cleanup: Result<(), Error>) -> Error {
    match cleanup {
        Ok(()) => error,
        Err(other) => Error::Failed(format!("{error}; {other}")),
    }
}

fn remove_tree<'a>(
    source: &'a Resource,
    cancel: &'a CancellationToken,
) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
    Box::pin(async move {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let kind = source.kind().await?;
        if kind == EntryKind::Directory {
            for child in source.children().await? {
                remove_tree(&child, cancel).await?;
            }
        }
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        source.remove(kind).await
    })
}
