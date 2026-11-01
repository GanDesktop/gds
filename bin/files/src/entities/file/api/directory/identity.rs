//! Owner and group resolution for one directory read.
//!
//! Every distinct uid/gid is resolved once per listing, so a directory with many
//! entries does not issue a name-service query per entry.

#[cfg(unix)]
mod unix;
#[cfg(not(unix))]
mod unsupported;

#[cfg(unix)]
pub(super) use unix::IdentityCache;
#[cfg(not(unix))]
pub(super) use unsupported::IdentityCache;
