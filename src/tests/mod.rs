use std::sync::Mutex;

pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());
