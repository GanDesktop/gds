/// Placeholder cache for platforms without Unix owner and group metadata.
///
/// Every lookup reports that the value is unknown, so callers keep the `None`
/// fields instead of inventing an owner, group or permission set.
#[derive(Debug, Default)]
pub(crate) struct IdentityCache;

impl IdentityCache {
    pub(crate) fn new() -> Self {
        Self
    }
}
