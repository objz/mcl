// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use std::sync::Mutex;

pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());
