// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::state;

pub(crate) fn reset() {
    state::WIZARD_STATE.lock().expect("wizard state").reset();
    *state::WIZARD_RESULT.lock().expect("wizard result") = None;
}
