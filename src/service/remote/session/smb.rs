use super::super::*;
use ::smb::{
    Client as SmbClient, ClientConfig, CreateOptions, Directory, FileAccessMask, FileAttributes,
    FileBasicInformation, FileCreateArgs, FileDirectoryInformation, FileDispositionInformation,
    FileRenameInformation, FileStandardInformation, Resource, ResourceHandle, UncPath,
};
use futures_lite::StreamExt;
use std::{collections::HashSet, future::Future, sync::Arc};
use tokio::sync::Mutex;

pub(super) struct Client {
    inner: SmbClient,
    server: String,
    user: String,
    credentials: Credentials,
    shares: Mutex<HashSet<String>>,
}

impl Client {
    pub(super) async fn connect(
        address: &Address,
        credentials: Credentials,
    ) -> Result<Self, Error> {
        let server = if address.host().contains(':') {
            format!("[{}]:{}", address.host(), address.port())
        } else if address.port() == 445 {
            address.host().to_owned()
        } else {
            format!("{}:{}", address.host(), address.port())
        };
        let user = address.username();
        let mut config = ClientConfig::default();
        config.connection.allow_unsigned_guest_access =
            user == "guest" && credentials.password.is_empty();
        let inner = SmbClient::new(config);
        let client = Self {
            inner,
            server,
            user,
            credentials,
            shares: Mutex::new(HashSet::new()),
        };
        if address.path() == "/" {
            Self::ensure_connected(&client.shares, "IPC$", || {
                client.connect_ipc("SMB authentication")
            })
            .await?;
        } else {
            let (share, unc) = client.parse_unc(address)?;
            Self::ensure_connected(&client.shares, &share, || client.connect_share(&unc)).await?;
        }
        Ok(client)
    }

    fn parse_unc(&self, address: &Address) -> Result<(String, UncPath), Error> {
        let path = address.path();
        let mut parts = path.trim_start_matches('/').splitn(2, '/');
        let share = parts
            .next()
            .filter(|v| !v.is_empty())
            .ok_or(Error::Unsupported("select an SMB share first"))?;
        let unc = UncPath::new(&self.server)
            .and_then(|v| v.with_share(share))
            .map_err(|e| Error::protocol("SMB path", e))?
            .with_path(parts.next().unwrap_or_default());
        Ok((share.to_owned(), unc))
    }

    async fn connect_ipc(&self, operation: &'static str) -> Result<(), Error> {
        self.inner
            .ipc_connect(&self.server, &self.user, self.credentials.password.clone())
            .await
            .map_err(|e| Error::protocol(operation, e))
    }

    async fn connect_share(&self, unc: &UncPath) -> Result<(), Error> {
        self.inner
            .share_connect(unc, &self.user, self.credentials.password.clone())
            .await
            .map_err(|e| Error::protocol("SMB share connection", e))
    }

    async fn ensure_connected<C, CFut>(
        shares: &Mutex<HashSet<String>>,
        share: &str,
        connect: C,
    ) -> Result<bool, Error>
    where
        C: FnOnce() -> CFut,
        CFut: Future<Output = Result<(), Error>>,
    {
        let mut cached = shares.lock().await;
        if cached.contains(share) {
            return Ok(true);
        }
        connect().await?;
        cached.insert(share.to_owned());
        Ok(false)
    }

    async fn evict_share(shares: &Mutex<HashSet<String>>, share: &str) {
        shares.lock().await.remove(share);
    }

    async fn with_share_retry<T, C, CFut, O, OFut>(
        shares: &Mutex<HashSet<String>>,
        share: &str,
        mut connect: C,
        mut op: O,
    ) -> Result<T, Error>
    where
        C: FnMut() -> CFut,
        CFut: Future<Output = Result<(), Error>>,
        O: FnMut() -> OFut,
        OFut: Future<Output = Result<T, Error>>,
    {
        let used_cached = Self::ensure_connected(shares, share, &mut connect).await?;
        match op().await {
            Ok(value) => Ok(value),
            Err(_) if used_cached => {
                // Evict stale tree/session state before attempting a single reconnect and retry.
                Self::evict_share(shares, share).await;
                Self::ensure_connected(shares, share, &mut connect).await?;
                op().await
            }
            Err(error) => Err(error),
        }
    }

    async fn create_file_with_retry(
        &self,
        share: &str,
        unc: &UncPath,
        operation: &'static str,
        args: &FileCreateArgs,
    ) -> Result<Resource, Error> {
        Self::with_share_retry(
            &self.shares,
            share,
            || self.connect_share(unc),
            || {
                let inner = &self.inner;
                async move {
                    inner
                        .create_file(unc, args)
                        .await
                        .map_err(|e| Error::protocol(operation, e))
                }
            },
        )
        .await
    }

    fn handle(resource: &Resource) -> &ResourceHandle {
        match resource {
            Resource::File(file) => file.handle(),
            Resource::Directory(dir) => dir.handle(),
            Resource::Pipe(pipe) => pipe.handle(),
        }
    }

    async fn finish<T>(resource: Resource, result: Result<T, Error>) -> Result<T, Error> {
        let closed = Self::handle(&resource)
            .close()
            .await
            .map_err(|e| Error::protocol("SMB close", e));
        match (result, closed) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
            (Err(error), Err(close)) => Err(Error::protocol(
                "SMB operation and close",
                anyhow::anyhow!("{error}; {close}"),
            )),
        }
    }

    async fn finish_on_share<T>(
        &self,
        share: &str,
        resource: Resource,
        result: Result<T, Error>,
    ) -> Result<T, Error> {
        let outcome = Self::finish(resource, result).await;
        if matches!(outcome, Err(Error::Protocol { .. })) {
            Self::evict_share(&self.shares, share).await;
        }
        outcome
    }

    pub(super) async fn list(&self, address: &Address) -> Result<Vec<Entry>, Error> {
        if address.path() == "/" {
            let listed = Self::with_share_retry(
                &self.shares,
                "IPC$",
                || self.connect_ipc("SMB share enumeration connection"),
                || {
                    let inner = &self.inner;
                    let server = &self.server;
                    async move {
                        inner
                            .list_shares(server)
                            .await
                            .map_err(|e| Error::ShareList(e.into()))
                    }
                },
            )
            .await?;
            return listed
                .into_iter()
                .filter_map(|v| v.netname.as_ref().map(|name| name.to_string()))
                .filter(|name| !name.ends_with('$'))
                .map(|name| {
                    Ok(Entry {
                        address: address.child(&name)?,
                        name,
                        kind: EntryKind::Directory,
                        size: None,
                        modified: None,
                    })
                })
                .collect();
        }
        let (share, unc) = self.parse_unc(address)?;
        let args =
            FileCreateArgs::make_open_existing(FileAccessMask::new().with_generic_read(true));
        let resource = self
            .create_file_with_retry(&share, &unc, "SMB list open", &args)
            .await?;
        let Resource::Directory(directory) = resource else {
            return self
                .finish_on_share(
                    &share,
                    resource,
                    Err(Error::Unsupported("location is not a directory")),
                )
                .await;
        };
        let directory = Arc::new(directory);
        let result = async {
            let mut query = std::pin::pin!(
                Directory::query::<FileDirectoryInformation>(&directory, "*")
                    .await
                    .map_err(|e| Error::protocol("SMB directory query", e))?
            );
            let mut entries = Vec::new();
            while let Some(item) = query.next().await {
                let item = item.map_err(|e| Error::protocol("SMB directory entry", e))?;
                let name = String::try_from(item.file_name)
                    .map_err(|e| Error::protocol("SMB filename", e))?;
                if matches!(name.as_str(), "." | "..") {
                    continue;
                }
                let kind = Self::kind(item.file_attributes);
                entries.push(Entry {
                    address: address.child(&name)?,
                    name,
                    kind,
                    size: (kind == EntryKind::File).then_some(item.end_of_file),
                    modified: (!item.last_write_time.is_zero())
                        .then(|| item.last_write_time.into()),
                });
            }
            Ok(entries)
        }
        .await;
        let directory = Arc::try_unwrap(directory)
            .map_err(|_| Error::Unsupported("SMB directory is still in use"))?;
        self.finish_on_share(&share, Resource::Directory(directory), result)
            .await
    }

    fn kind(attributes: FileAttributes) -> EntryKind {
        if attributes.reparse_point() {
            EntryKind::SymbolicLink
        } else if attributes.directory() {
            EntryKind::Directory
        } else {
            EntryKind::File
        }
    }

    fn stat_size(kind: EntryKind, end_of_file: u64) -> Option<u64> {
        (kind == EntryKind::File).then_some(end_of_file)
    }

    pub(super) async fn stat(&self, address: &Address) -> Result<Entry, Error> {
        let (share, unc) = self.parse_unc(address)?;
        let mut args = FileCreateArgs::make_open_existing(
            FileAccessMask::new().with_file_read_attributes(true),
        );
        args.options = args.options.with_open_reparse_point(true);
        let resource = self
            .create_file_with_retry(&share, &unc, "SMB metadata open", &args)
            .await?;
        let result = async {
            let basic = Self::handle(&resource)
                .query_info::<FileBasicInformation>()
                .await
                .map_err(|e| Error::protocol("SMB metadata", e))?;
            let kind = Self::kind(basic.file_attributes);
            let size = if kind == EntryKind::File {
                let standard = Self::handle(&resource)
                    .query_info::<FileStandardInformation>()
                    .await
                    .map_err(|e| Error::protocol("SMB size", e))?;
                Self::stat_size(kind, standard.end_of_file)
            } else {
                None
            };
            Ok(Entry {
                address: address.clone(),
                name: address.name(),
                kind,
                size,
                modified: (!basic.last_write_time.is_zero()).then(|| basic.last_write_time.into()),
            })
        }
        .await;
        self.finish_on_share(&share, resource, result).await
    }
    pub(super) async fn mkdir(&self, address: &Address) -> Result<(), Error> {
        let (share, unc) = self.parse_unc(address)?;
        let args = FileCreateArgs::make_create_new(
            FileAttributes::new().with_directory(true),
            CreateOptions::new().with_directory_file(true),
        );
        let resource = self
            .create_file_with_retry(&share, &unc, "SMB mkdir", &args)
            .await?;
        self.finish_on_share(&share, resource, Ok(())).await
    }
    pub(super) async fn rename(&self, from: &Address, to: &Address) -> Result<(), Error> {
        let (share, from) = self.parse_unc(from)?;
        let (to_share, to) = self.parse_unc(to)?;
        if share != to_share {
            return Err(Error::Unsupported("rename across SMB shares"));
        }
        let mut args = FileCreateArgs::make_open_existing(FileAccessMask::new().with_delete(true));
        args.options = args.options.with_open_reparse_point(true);
        let resource = self
            .create_file_with_retry(&share, &from, "SMB rename open", &args)
            .await?;
        let result = Self::handle(&resource)
            .set_info(FileRenameInformation {
                replace_if_exists: false.into(),
                root_directory: 0,
                file_name: to.path().unwrap_or_default().into(),
            })
            .await
            .map_err(|e| Error::protocol("SMB rename", e));
        self.finish_on_share(&share, resource, result).await
    }

    fn remove_args(kind: EntryKind) -> FileCreateArgs {
        let mut args = FileCreateArgs::make_open_existing(FileAccessMask::new().with_delete(true));
        let options = args.options.with_open_reparse_point(true);
        args.options = if kind == EntryKind::Directory {
            options.with_directory_file(true)
        } else {
            options.with_non_directory_file(true)
        };
        args
    }

    pub(super) async fn remove(&self, address: &Address, kind: EntryKind) -> Result<(), Error> {
        let (share, unc) = self.parse_unc(address)?;
        let args = Self::remove_args(kind);
        let resource = self
            .create_file_with_retry(&share, &unc, "SMB delete open", &args)
            .await?;
        let result = Self::handle(&resource)
            .set_info(FileDispositionInformation::default())
            .await
            .map_err(|e| Error::protocol("SMB delete", e));
        self.finish_on_share(&share, resource, result).await
    }
    pub(super) async fn open(
        &self,
        address: &Address,
        create: bool,
    ) -> Result<super::Stream, Error> {
        let (share, unc) = self.parse_unc(address)?;
        let args = if create {
            FileCreateArgs::make_create_new(
                FileAttributes::new(),
                CreateOptions::new().with_non_directory_file(true),
            )
        } else {
            FileCreateArgs::make_open_existing(FileAccessMask::new().with_generic_read(true))
        };
        let resource = self
            .create_file_with_retry(&share, &unc, "SMB file open", &args)
            .await?;
        match resource {
            Resource::File(file) => Ok(super::Stream::smb(file)),
            other => {
                self.finish_on_share(&share, other, Err(Error::Unsupported("not a regular file")))
                    .await
            }
        }
    }
    pub(super) async fn close(&self) -> Result<(), Error> {
        self.shares.lock().await.clear();
        self.inner
            .close()
            .await
            .map_err(|e| Error::protocol("SMB disconnect", e))
    }
}

#[cfg(test)]
mod tests;
