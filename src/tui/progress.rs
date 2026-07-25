// global progress state shared between background tasks and the status bar widget.
// background tasks set the action/progress, the render loop reads it every frame.

use std::collections::BTreeMap;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Default, Clone)]
pub struct ProgressState {
    pub current_action: Option<String>,
    pub progress: Option<(u64, u64)>,
    pub sub_action: Option<String>,
    tasks: BTreeMap<u64, TaskState>,
    legacy_active: bool,
}

#[derive(Debug, Clone)]
struct TaskState {
    action: String,
    sub_action: Option<String>,
    progress: Option<(u64, u64)>,
}

pub static PROGRESS: LazyLock<Arc<Mutex<ProgressState>>> =
    LazyLock::new(|| Arc::new(Mutex::new(ProgressState::default())));
static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

pub struct ProgressTask {
    id: u64,
    finished: bool,
}

#[derive(Clone)]
pub struct ProgressTaskHandle {
    id: u64,
}

impl ProgressTask {
    pub fn start(action: impl Into<String>) -> Self {
        let id = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed);
        let action = action.into();
        if let Ok(mut state) = PROGRESS.lock() {
            state.tasks.insert(
                id,
                TaskState {
                    action: action.clone(),
                    sub_action: None,
                    progress: None,
                },
            );
            refresh_visible(&mut state);
        }
        tracing::info!("{}", action);
        crate::tui::request_redraw();
        Self {
            id,
            finished: false,
        }
    }

    pub fn handle(&self) -> ProgressTaskHandle {
        ProgressTaskHandle { id: self.id }
    }

    pub fn set_action(&self, text: impl Into<String>) {
        update_task_action(self.id, text.into());
    }

    pub fn set_sub_action(&self, text: impl Into<String>) {
        update_task_sub_action(self.id, text.into());
    }

    pub fn set_progress(&self, current: u64, total: u64) {
        update_task_progress(self.id, current, total);
    }

    pub fn finish(mut self) {
        self.remove();
        self.finished = true;
    }

    pub fn fail(mut self, error: impl std::fmt::Display) {
        tracing::warn!("Progress task failed: {error}");
        self.remove();
        self.finished = true;
    }

    fn remove(&self) {
        if let Ok(mut state) = PROGRESS.lock() {
            state.tasks.remove(&self.id);
            refresh_visible(&mut state);
        }
        crate::tui::request_redraw();
    }
}

impl ProgressTaskHandle {
    pub fn set_sub_action(&self, text: impl Into<String>) {
        update_task_sub_action(self.id, text.into());
    }

    pub fn set_progress(&self, current: u64, total: u64) {
        update_task_progress(self.id, current, total);
    }
}

fn update_task_action(id: u64, text: String) {
    if let Ok(mut state) = PROGRESS.lock() {
        if let Some(task) = state.tasks.get_mut(&id) {
            task.action = text;
        }
        refresh_visible(&mut state);
    }
    crate::tui::request_redraw();
}

fn update_task_sub_action(id: u64, text: String) {
    if let Ok(mut state) = PROGRESS.lock() {
        if let Some(task) = state.tasks.get_mut(&id) {
            task.sub_action = Some(text);
        }
        refresh_visible(&mut state);
    }
    crate::tui::request_redraw();
}

fn update_task_progress(id: u64, current: u64, total: u64) {
    if let Ok(mut state) = PROGRESS.lock() {
        if let Some(task) = state.tasks.get_mut(&id) {
            task.progress = Some((current, total));
        }
        refresh_visible(&mut state);
    }
    crate::tui::request_redraw();
}

impl Drop for ProgressTask {
    fn drop(&mut self) {
        if !self.finished {
            self.remove();
        }
    }
}

fn refresh_visible(state: &mut ProgressState) {
    if let Some((_, task)) = state.tasks.last_key_value() {
        state.current_action = Some(task.action.clone());
        state.sub_action.clone_from(&task.sub_action);
        state.progress = task.progress;
    } else if !state.legacy_active {
        state.current_action = None;
        state.sub_action = None;
        state.progress = None;
    }
}

pub fn set_action(text: impl Into<String>) {
    let text = text.into();
    match PROGRESS.lock() {
        Ok(mut state) => {
            state.legacy_active = true;
            state.current_action = Some(text.clone());
            crate::tui::request_redraw();
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
            crate::tui::request_redraw();
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
            crate::tui::request_redraw();
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
            state.legacy_active = false;
            refresh_visible(&mut state);
            crate::tui::request_redraw();
        }
        Err(e) => {
            tracing::error!("Progress lock poisoned: {}", e);
        }
    }
}

pub fn is_active() -> bool {
    PROGRESS
        .lock()
        .is_ok_and(|state| state.current_action.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn newest_task_is_visible_and_drop_restores_previous_task() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear();
        let first = ProgressTask::start("first");
        first.set_progress(1, 2);
        {
            let second = ProgressTask::start("second");
            second.set_sub_action("working");
            let state = PROGRESS.lock().unwrap();
            assert_eq!(state.current_action.as_deref(), Some("second"));
            assert_eq!(state.sub_action.as_deref(), Some("working"));
        }
        let state = PROGRESS.lock().unwrap();
        assert_eq!(state.current_action.as_deref(), Some("first"));
        assert_eq!(state.progress, Some((1, 2)));
        drop(state);
        first.finish();
        clear();
    }

    #[test]
    fn changing_phase_keeps_the_existing_progress_visible() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear();
        let task = ProgressTask::start("inventory");
        task.set_progress(4, 10);
        task.set_action("provider matching");

        let state = PROGRESS.lock().unwrap();
        assert_eq!(state.current_action.as_deref(), Some("provider matching"));
        assert_eq!(state.progress, Some((4, 10)));
        drop(state);

        task.finish();
        assert!(!is_active());
        clear();
    }

    #[test]
    fn clearing_legacy_progress_keeps_owned_tasks() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear();
        let task = ProgressTask::start("indexing");
        set_action("legacy download");
        clear();

        let state = PROGRESS.lock().unwrap();
        assert_eq!(state.current_action.as_deref(), Some("indexing"));
        drop(state);

        task.finish();
        clear();
    }
}
