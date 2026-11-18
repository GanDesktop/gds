//! Decoration policy, independent of input, redraws and application work.
use anti_webui::prelude::Transition;

pub const WORKSPACE: Transition = Transition::ms(160).ease_out();
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Policy {
    pub enabled: bool,
    pub system_reduced: bool,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: true,
            system_reduced: false,
        }
    }
}
impl Policy {
    pub fn reduced(self) -> bool {
        !self.enabled || self.system_reduced
    }
}
pub fn enabled() -> bool {
    crate::app::Gds::current().appearance().motion().enabled()
}

#[cfg(test)]
mod tests;
