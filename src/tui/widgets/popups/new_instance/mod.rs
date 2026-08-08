mod render;
mod state;

pub use super::LoadState;
pub(crate) use render::render_select_list;
pub use render::{popup_rect, render};
pub use state::{WizardParams, WizardState, WizardStep, handle_key, take_result};

#[cfg(test)]
#[path = "../../../tests/widgets/popups/new_instance/support.rs"]
mod test_support;
#[cfg(test)]
pub(crate) use test_support::reset as reset_for_test;
