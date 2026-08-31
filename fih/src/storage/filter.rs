use alloc::string::String;
use alloc::vec::Vec;

use crate::fih::BoardState;
use crate::storage::read::StorageRead;

/// Axis hints for CoordSpaceN prefix queries.
/// When provided, enables O(subtree) iter_prefix instead of full scan.
/// Axis convention: [0]=time_hi, [1]=time_lo, [2]=entity, [3]=origin, [4]=creator, [5]=serial
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AxisHints {
    pub time_hi: Option<u16>,
    pub time_lo: Option<u16>,
    pub entity: Option<u16>,
    pub origin: Option<u16>,
    pub creator: Option<u16>,
    pub serial: Option<u16>,
}

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
    pub origin: Option<String>,
    pub creator: Option<String>,
    pub status: Option<String>,
    /// Axis hints for CoordSpaceN prefix queries.
    ///
    /// Reserved: the concept is part of the spatial query surface (leading
    /// axis prefix pruning) and is preserved as designed. The current
    /// FihStorage implementation scans the record layer with
    /// record-field predicates; the structural filter index is maintained
    /// for spatial queries and does not consume them yet; wiring the
    /// prefix-query path is a pending optimization, not a removal.
    pub axis_hints: Option<AxisHints>,
}

/// Backend supports filtered/partial reads.
pub trait FilterCapable: StorageRead {
    fn read_state_filtered(&self, filter: &StateFilter) -> BoardState;
}
