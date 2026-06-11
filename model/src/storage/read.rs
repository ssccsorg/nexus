use crate::fih::BoardState;

/// Core storage trait. Every backend must implement at least this.
pub trait StorageRead {
    fn project_id(&self) -> &str;
    fn read_state(&self) -> BoardState;
}

impl<T: StorageRead> StorageRead for &T {
    fn project_id(&self) -> &str {
        (**self).project_id()
    }
    fn read_state(&self) -> BoardState {
        (**self).read_state()
    }
}

impl<T: StorageRead> StorageRead for &mut T {
    fn project_id(&self) -> &str {
        (**self).project_id()
    }
    fn read_state(&self) -> BoardState {
        (**self).read_state()
    }
}
