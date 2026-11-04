//! Files application startup and native window lifecycle.

mod startup;
mod windows;
pub(crate) use startup::run;

pub(crate) mod cli;
