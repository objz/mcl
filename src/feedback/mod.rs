// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// application feedback shared by background work and every frontend.

use std::sync::atomic::{AtomicBool, Ordering};

pub mod errors;
pub mod progress;

static REDRAW_REQUESTED: AtomicBool = AtomicBool::new(true);

pub fn request_redraw() {
    REDRAW_REQUESTED.store(true, Ordering::Release);
}

pub(crate) fn take_redraw_request() -> bool {
    REDRAW_REQUESTED.swap(false, Ordering::AcqRel)
}
