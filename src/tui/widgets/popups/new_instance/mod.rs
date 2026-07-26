mod render;
mod state;

pub(crate) use render::render_select_list;
pub use render::{popup_rect, render};
pub use state::{LoadState, WizardParams, WizardState, WizardStep, handle_key, take_result};

#[cfg(test)]
pub(crate) fn reset_for_test() {
    state::WIZARD_STATE.lock().expect("wizard state").reset();
    *state::WIZARD_RESULT.lock().expect("wizard result") = None;
}
