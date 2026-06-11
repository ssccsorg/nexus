use super::read::StorageRead;
use crate::error::BlackboardError;
use crate::fih::Hint;

/// Backend can accept Hints.
pub trait HintCapable: StorageRead {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError>;
}

impl<T: HintCapable> HintCapable for &T {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        (**self).submit_hint(hint)
    }
}

impl<T: HintCapable> HintCapable for &mut T {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        (**self).submit_hint(hint)
    }
}
