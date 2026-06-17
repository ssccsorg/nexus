use super::read::StorageRead;
use crate::fih::BoardState;

/// Filter for partial reads. All fields are optional; omitted fields
/// mean "no filtering on this dimension".
#[derive(Debug, Clone, Default)]
pub struct StateFilter {
    pub fact_ids: Option<Vec<String>>,
    pub intent_ids: Option<Vec<String>>,
    pub hint_ids: Option<Vec<String>>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub creator: Option<String>,
    pub status: Option<String>,
}

/// Backend supports filtered/partial reads.
pub trait FilterCapable: StorageRead {
    fn read_state_filtered(&self, filter: &StateFilter) -> BoardState;
}
