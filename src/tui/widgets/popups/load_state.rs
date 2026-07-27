#[derive(Debug, Clone, Default)]
pub enum LoadState<T> {
    #[default]
    Idle,
    Loading,
    Loaded(T),
    Error(String),
}
