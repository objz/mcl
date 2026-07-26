mod flows;
pub(super) mod harness;
mod snapshots;

use std::sync::Mutex;

pub(crate) static UI_TEST_LOCK: Mutex<()> = Mutex::new(());
