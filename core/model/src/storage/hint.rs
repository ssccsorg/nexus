use super::read::StorageRead;
use crate::error::BlackboardError;
use crate::fih::Hint;

/// Backend can accept Hints.
pub trait HintCapable: StorageRead {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError>;
}
