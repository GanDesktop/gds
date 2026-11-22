//! GDS desktop portal backend.

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod chooser;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod protocol;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[derive(clap::Parser)]
#[command(
    name = "xdg-desktop-portal-gds",
    version,
    about = "GDS desktop portal backend"
)]
struct Args {
    /// Executable implementing the native GDS file chooser protocol.
    #[arg(long, default_value = "gds-files")]
    file_chooser: std::path::PathBuf,
}

/// # Errors
/// Reports unavailable D-Bus connections, name ownership or signal handling.
// Standalone headless D-Bus daemon (does not run GPUI or `Supervisor`).
#[allow(clippy::disallowed_methods)]
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use anyhow::Context as _;
    use clap::Parser;
    use tokio::signal::unix::{SignalKind, signal};

    let args = Args::parse();
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let chooser = chooser::FileChooser::new(args.file_chooser.into_os_string());
    let connection = zbus::connection::Builder::session()?
        .serve_at(chooser::OBJECT_PATH, chooser.clone())?
        .build()
        .await?;
    connection.request_name(chooser::BUS_NAME).await?;
    let stopped = tokio::select! {
        stopped = terminate.recv() => stopped,
        stopped = interrupt.recv() => stopped,
    };
    chooser.shutdown().await;
    connection.graceful_shutdown().await;
    stopped.context("signal stream closed")?;
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn main() {
    eprintln!("GDS Portal is only available on Linux and FreeBSD.");
    std::process::exit(1);
}
