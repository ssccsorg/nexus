use super::read::StorageRead;
use crate::error::BlackboardError;
use crate::fih::{Fact, FihHash};

/// Backend can accept Facts.
pub trait FactCapable: StorageRead {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError>;
}
