mod render;
mod state;

pub use render::{popup_rect, render};
pub use state::{ImportResult, ImportStep, ImportWizardState, handle_key, take_result};

#[cfg(test)]
pub(crate) fn reset_for_test() {
    state::IMPORT_STATE
        .lock()
        .expect("import wizard state")
        .reset();
    *state::IMPORT_RESULT.lock().expect("import wizard result") = None;
}
