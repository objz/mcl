use super::state;

pub(crate) fn reset() {
    state::WIZARD_STATE.lock().expect("wizard state").reset();
    *state::WIZARD_RESULT.lock().expect("wizard result") = None;
}
