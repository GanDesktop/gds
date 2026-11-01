use super::super::*;
use ::smb::{
    Client as SmbClient, ClientConfig, CreateOptions, Directory, FileAccessMask, FileAttributes,
    FileBasicInformation, FileCreateArgs, FileDirectoryInformation, FileDispositionInformation,
    FileRenameInformation, FileStandardInformation, Resource, ResourceHandle, UncPath,
};
use futures_lite::StreamExt;
use std::{collections::HashSet, sync::Arc};
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
            client
                .inner
                .ipc_connect(
                    &client.server,
                    &client.user,
                    client.credentials.password.clone(),
                )
                .await
                .map_err(|e| Error::protocol("SMB authentication", e))?;
            client.shares.lock().await.insert("IPC$".into());
        } else {
            client.unc(address).await?;
        }
        Ok(client)
    }

    async fn unc(&self, address: &Address) -> Result<UncPath, Error> {
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
        let mut shares = self.shares.lock().await;
        if !shares.contains(share) {
            self.inner
                .share_connect(&unc, &self.user, self.credentials.password.clone())
                .await
                .map_err(|e| Error::protocol("SMB share connection", e))?;
            shares.insert(share.into());
        }
        Ok(unc)
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

    pub(super) async fn list(&self, address: &Address) -> Result<Vec<Entry>, Error> {
        if address.path() == "/" {
            let mut shares = self.shares.lock().await;
            if !shares.contains("IPC$") {
                self.inner
                    .ipc_connect(&self.server, &self.user, self.credentials.password.clone())
                    .await
                    .map_err(|e| Error::protocol("SMB share enumeration connection", e))?;
                shares.insert("IPC$".into());
            }
            drop(shares);
            return self
                .inner
                .list_shares(&self.server)
                .await
                .map_err(|e| Error::ShareList(e.into()))?
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
        let unc = self.unc(address).await?;
        let args =
            FileCreateArgs::make_open_existing(FileAccessMask::new().with_generic_read(true));
        let resource = self
            .inner
            .create_file(&unc, &args)
            .await
            .map_err(|e| Error::protocol("SMB list open", e))?;
        let Resource::Directory(directory) = resource else {
            return Self::finish(
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
        Self::finish(Resource::Directory(directory), result).await
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
    pub(super) async fn stat(&self, address: &Address) -> Result<Entry, Error> {
        let unc = self.unc(address).await?;
        let mut args = FileCreateArgs::make_open_existing(
            FileAccessMask::new().with_file_read_attributes(true),
        );
        args.options = args.options.with_open_reparse_point(true);
        let resource = self
            .inner
            .create_file(&unc, &args)
            .await
            .map_err(|e| Error::protocol("SMB metadata open", e))?;
        let result = async {
            let basic = Self::handle(&resource)
                .query_info::<FileBasicInformation>()
                .await
                .map_err(|e| Error::protocol("SMB metadata", e))?;
            let standard = Self::handle(&resource)
                .query_info::<FileStandardInformation>()
                .await
                .map_err(|e| Error::protocol("SMB size", e))?;
            Ok(Entry {
                address: address.clone(),
                name: address.name(),
                kind: Self::kind(basic.file_attributes),
                size: Some(standard.end_of_file),
                modified: (!basic.last_write_time.is_zero()).then(|| basic.last_write_time.into()),
            })
        }
        .await;
        Self::finish(resource, result).await
    }
    pub(super) async fn mkdir(&self, address: &Address) -> Result<(), Error> {
        let unc = self.unc(address).await?;
        let args = FileCreateArgs::make_create_new(
            FileAttributes::new().with_directory(true),
            CreateOptions::new().with_directory_file(true),
        );
        let resource = self
            .inner
            .create_file(&unc, &args)
            .await
            .map_err(|e| Error::protocol("SMB mkdir", e))?;
        Self::finish(resource, Ok(())).await
    }
    pub(super) async fn rename(&self, from: &Address, to: &Address) -> Result<(), Error> {
        let from = self.unc(from).await?;
        let to = self.unc(to).await?;
        if from.share() != to.share() {
            return Err(Error::Unsupported("rename across SMB shares"));
        }
        let mut args = FileCreateArgs::make_open_existing(FileAccessMask::new().with_delete(true));
        args.options = args.options.with_open_reparse_point(true);
        let resource = self
            .inner
            .create_file(&from, &args)
            .await
            .map_err(|e| Error::protocol("SMB rename open", e))?;
        let result = Self::handle(&resource)
            .set_info(FileRenameInformation {
                replace_if_exists: false.into(),
                root_directory: 0,
                file_name: to.path().unwrap_or_default().into(),
            })
            .await
            .map_err(|e| Error::protocol("SMB rename", e));
        Self::finish(resource, result).await
    }
    pub(super) async fn remove(&self, address: &Address, _: EntryKind) -> Result<(), Error> {
        let unc = self.unc(address).await?;
        let mut args = FileCreateArgs::make_open_existing(FileAccessMask::new().with_delete(true));
        args.options = args.options.with_open_reparse_point(true);
        let resource = self
            .inner
            .create_file(&unc, &args)
            .await
            .map_err(|e| Error::protocol("SMB delete open", e))?;
        let result = Self::handle(&resource)
            .set_info(FileDispositionInformation::default())
            .await
            .map_err(|e| Error::protocol("SMB delete", e));
        Self::finish(resource, result).await
    }
    pub(super) async fn open(
        &self,
        address: &Address,
        create: bool,
    ) -> Result<super::Stream, Error> {
        let unc = self.unc(address).await?;
        let args = if create {
            FileCreateArgs::make_create_new(
                FileAttributes::new(),
                CreateOptions::new().with_non_directory_file(true),
            )
        } else {
            FileCreateArgs::make_open_existing(FileAccessMask::new().with_generic_read(true))
        };
        let resource = self
            .inner
            .create_file(&unc, &args)
            .await
            .map_err(|e| Error::protocol("SMB file open", e))?;
        match resource {
            Resource::File(file) => Ok(super::Stream::smb(file)),
            other => Self::finish(other, Err(Error::Unsupported("not a regular file"))).await,
        }
    }
    pub(super) async fn close(&self) -> Result<(), Error> {
        self.inner
            .close()
            .await
            .map_err(|e| Error::protocol("SMB disconnect", e))
    }
}
