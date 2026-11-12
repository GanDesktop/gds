//! GDS Files: native directory browsing and file selection.

mod app;
mod entities;
mod features;
mod pages;
mod shared;
mod widgets;

fn main() -> std::process::ExitCode {
    app::run()
}
