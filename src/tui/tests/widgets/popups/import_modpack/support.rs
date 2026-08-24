// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::state;

pub(crate) fn reset() {
    state::IMPORT_STATE
        .lock()
        .expect("import wizard state")
        .reset();
    *state::IMPORT_RESULT.lock().expect("import wizard result") = None;
    *state::DISCOVERY_STATE
        .lock()
        .expect("modpack discovery state") =
        crate::tui::widgets::content::DiscoveryState::new_modpacks();
}
