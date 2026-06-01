use crate::fih::BoardState;

/// Core storage trait. Every backend must implement at least this.
pub trait StorageRead: Send + Sync {
    fn project_id(&self) -> &str;
    fn read_state(&self) -> BoardState;
}
