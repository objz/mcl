// global progress state shared between background tasks and the status bar widget.
// background tasks set the action/progress, the render loop reads it every frame.

use std::sync::LazyLock;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default, Clone)]
pub struct ProgressState {
    pub current_action: Option<String>,
    pub progress: Option<(u64, u64)>,
    pub sub_action: Option<String>,
}

pub static PROGRESS: LazyLock<Arc<Mutex<ProgressState>>> =
    LazyLock::new(|| Arc::new(Mutex::new(ProgressState::default())));

pub fn set_action(text: impl Into<String>) {
    let text = text.into();
    match PROGRESS.lock() {
        Ok(mut state) => {
            state.current_action = Some(text.clone());
        }
        Err(e) => {
            tracing::error!("Progress lock poisoned: {}", e);
        }
    }
    tracing::info!("{}", text);
}

pub fn set_progress(current: u64, total: u64) {
    match PROGRESS.lock() {
        Ok(mut state) => {
            state.progress = Some((current, total));
        }
        Err(e) => {
            tracing::error!("Progress lock poisoned: {}", e);
        }
    }
}

pub fn set_sub_action(text: impl Into<String>) {
    let text = text.into();
    match PROGRESS.lock() {
        Ok(mut state) => {
            state.sub_action = Some(text.clone());
        }
        Err(e) => {
            tracing::error!("Progress lock poisoned: {}", e);
        }
    }
    tracing::debug!("  {}", text);
}

pub fn clear() {
    match PROGRESS.lock() {
        Ok(mut state) => {
            state.current_action = None;
            state.progress = None;
            state.sub_action = None;
        }
        Err(e) => {
            tracing::error!("Progress lock poisoned: {}", e);
        }
    }
}
