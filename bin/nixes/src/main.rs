//! Flake-based Home Manager browser.
mod app;
mod entities;
mod pages;
mod shared;

fn main() -> anyhow::Result<()> {
    app::run()
}
