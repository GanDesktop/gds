//! GDS Agents: native session management and agent conversations for Bas.

mod app;
mod entities;
mod features;
mod pages;
mod shared;
mod widgets;

fn main() -> anyhow::Result<()> {
    app::run()
}
