//! Layer 3 workspace navigation, enrichment, remote loading, tab management, and Quick Access.

mod enrichment;
mod quick_access;
mod remote;
mod tabs;

pub(in crate::pages::workspace) use enrichment::EnrichmentRunner;
pub(in crate::pages::workspace) use quick_access::QuickAccess;
pub(in crate::pages::workspace) use tabs::Navigator;

#[cfg(test)]
mod tests;
