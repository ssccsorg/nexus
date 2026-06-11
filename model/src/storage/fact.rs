use super::read::StorageRead;
use crate::error::BlackboardError;
use crate::fih::{Fact, FihHash};

/// Backend can accept Facts.
pub trait FactCapable: StorageRead {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError>;
}

impl<T: FactCapable> FactCapable for &T {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        (**self).submit_fact(fact)
    }
}

impl<T: FactCapable> FactCapable for &mut T {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        (**self).submit_fact(fact)
    }
}
