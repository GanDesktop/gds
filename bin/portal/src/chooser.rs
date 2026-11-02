//! D-Bus request lifecycle and native picker processes.

use std::{
    collections::HashMap,
    ffi::OsString,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context as _, bail};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    process::{Child, Command},
    sync::watch,
};
use zbus::{Connection, zvariant::OwnedObjectPath};

use crate::protocol::{Options, Picker, Response};

pub const BUS_NAME: &str = "org.freedesktop.impl.portal.desktop.gds";
pub const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";
const TERMINATE_TIMEOUT: Duration = Duration::from_secs(3);

struct Request {
    cancel: watch::Sender<bool>,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
impl Request {
    fn close(&self) {
        self.cancel.send_replace(true);
    }
}

#[derive(Clone)]
struct Pending {
    cancel: watch::Sender<bool>,
    finished: watch::Receiver<bool>,
}

#[derive(Default)]
struct Requests {
    stopping: bool,
    active: HashMap<OwnedObjectPath, Pending>,
}

#[derive(Clone)]
pub struct FileChooser {
    executable: Arc<OsString>,
    requests: Arc<Mutex<Requests>>,
}

struct Invocation {
    handle: OwnedObjectPath,
    app_id: String,
    parent_window: String,
    title: String,
    options: Options,
}

enum Outcome<T> {
    Done(T),
    Cancelled,
}

impl FileChooser {
    pub fn new(executable: OsString) -> Self {
        Self {
            executable: Arc::new(executable),
            requests: Arc::default(),
        }
    }

    pub async fn shutdown(&self) {
        let pending = {
            let mut requests = self.requests.lock().expect("portal request state");
            requests.stopping = true;
            requests.active.values().cloned().collect::<Vec<_>>()
        };
        for request in &pending {
            request.cancel.send_replace(true);
        }
        for mut request in pending {
            let _ = request.finished.wait_for(|done| *done).await;
        }
    }

    async fn dispatch(
        &self,
        connection: &Connection,
        member: &str,
        invocation: Invocation,
    ) -> Response {
        let (cancel, cancelled) = watch::channel(false);
        let (finished, completion) = watch::channel(false);
        match connection
            .object_server()
            .at(
                &invocation.handle,
                Request {
                    cancel: cancel.clone(),
                },
            )
            .await
        {
            Ok(true) => (),
            Ok(false) => {
                eprintln!("FileChooser request already exists: {}", invocation.handle);
                return (2, Options::new());
            }
            Err(error) => {
                eprintln!("Could not export FileChooser request: {error}");
                return (2, Options::new());
            }
        }
        let accepted = {
            let mut requests = self.requests.lock().expect("portal request state");
            if requests.stopping {
                false
            } else {
                requests.active.insert(
                    invocation.handle.clone(),
                    Pending {
                        cancel,
                        finished: completion,
                    },
                );
                true
            }
        };
        let response = if accepted {
            match self.run(connection, member, &invocation, &cancelled).await {
                Ok(response) => response,
                Err(error) => {
                    eprintln!("FileChooser request failed: {error:#}");
                    (2, Options::new())
                }
            }
        } else {
            (1, Options::new())
        };
        if let Err(error) = connection
            .object_server()
            .remove::<Request, _>(&invocation.handle)
            .await
        {
            eprintln!("Could not remove FileChooser request: {error}");
        }
        self.requests
            .lock()
            .expect("portal request state")
            .active
            .remove(&invocation.handle);
        finished.send_replace(true);
        response
    }

    /// # Errors
    /// Reports invalid requests, picker failures or invalid output.
    async fn run(
        &self,
        _connection: &Connection,
        member: &str,
        invocation: &Invocation,
        cancelled: &watch::Receiver<bool>,
    ) -> anyhow::Result<Response> {
        let picker = Picker::from_invocation(
            member,
            &invocation.app_id,
            &invocation.parent_window,
            &invocation.title,
            &invocation.options,
        )?;
        self.pick(&picker, cancelled).await
    }

    async fn cancelled(cancelled: &watch::Receiver<bool>) {
        let _ = cancelled.clone().wait_for(|cancelled| *cancelled).await;
    }

    /// # Errors
    /// Reports process failures, invalid results, or failed process cleanup.
    async fn pick(
        &self,
        picker: &Picker,
        cancelled: &watch::Receiver<bool>,
    ) -> anyhow::Result<Response> {
        let mut child = Command::new(self.executable.as_ref())
            .arg("--portal-request")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let input = serde_json::to_vec(&picker.request)?;
        let mut stdin = child.stdin.take().context("picker stdin is not piped")?;
        let mut stdout = child.stdout.take().context("picker stdout is not piped")?;
        let mut output = Vec::new();
        let outcome = tokio::select! {
            result = async {
                let write = async move {
                    stdin.write_all(&input).await?;
                    drop(stdin);
                    Ok(())
                };
                let (status, read, write) = tokio::join!(
                    child.wait(), stdout.read_to_end(&mut output), write,
                );
                status.and_then(|status| read.and(write).map(|_| status))
            } => Outcome::Done(result),
            () = Self::cancelled(cancelled) => Outcome::Cancelled,
        };
        match outcome {
            Outcome::Cancelled => {
                Self::terminate(&mut child).await?;
                Ok((1, Options::new()))
            }
            Outcome::Done(status) => {
                let status = status?;
                if status.code() == Some(1) {
                    return Ok((1, Options::new()));
                }
                if !status.success() {
                    bail!("gds-files exited with {status}");
                }
                Ok((0, picker.results(&output)?))
            }
        }
    }

    /// # Errors
    /// Reports failures signalling or reaping the picker process.
    async fn terminate(child: &mut Child) -> anyhow::Result<()> {
        use nix::{
            errno::Errno,
            sys::signal::{Signal, kill},
            unistd::Pid,
        };

        let Some(id) = child.id() else { return Ok(()) };
        match kill(Pid::from_raw(id as i32), Signal::SIGTERM) {
            Ok(()) | Err(Errno::ESRCH) => (),
            Err(error) => return Err(error.into()),
        }
        match tokio::time::timeout(TERMINATE_TIMEOUT, child.wait()).await {
            Ok(status) => {
                status?;
            }
            Err(_) => child.kill().await?,
        }
        Ok(())
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl FileChooser {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        4
    }

    async fn open_file(
        &self,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        title: String,
        options: Options,
        #[zbus(connection)] connection: &Connection,
    ) -> Response {
        self.dispatch(
            connection,
            "OpenFile",
            Invocation {
                handle,
                app_id,
                parent_window,
                title,
                options,
            },
        )
        .await
    }

    async fn save_file(
        &self,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        title: String,
        options: Options,
        #[zbus(connection)] connection: &Connection,
    ) -> Response {
        self.dispatch(
            connection,
            "SaveFile",
            Invocation {
                handle,
                app_id,
                parent_window,
                title,
                options,
            },
        )
        .await
    }

    async fn save_files(
        &self,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        title: String,
        options: Options,
        #[zbus(connection)] connection: &Connection,
    ) -> Response {
        self.dispatch(
            connection,
            "SaveFiles",
            Invocation {
                handle,
                app_id,
                parent_window,
                title,
                options,
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests;
