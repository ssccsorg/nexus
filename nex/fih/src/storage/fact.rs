use crate::error::BlackboardError;
use crate::fih::{CoordId, Fact};
use crate::storage::read::StorageRead;

/// Backend can accept Facts.
pub trait FactCapable: StorageRead {
    fn submit_fact(&self, fact: &Fact) -> Result<CoordId, BlackboardError>;
}

impl<T: FactCapable> FactCapable for &T {
    fn submit_fact(&self, fact: &Fact) -> Result<CoordId, BlackboardError> {
        (**self).submit_fact(fact)
    }
}

impl<T: FactCapable> FactCapable for &mut T {
    fn submit_fact(&self, fact: &Fact) -> Result<CoordId, BlackboardError> {
        (**self).submit_fact(fact)
    }
}
