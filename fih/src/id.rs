// ── The FIH identity over a tagma coordinate ─────────────────────────
//
// The identity type is the coordinate model's: `tagma_core::CoordId` prints,
// parses, orders and hashes a path. This module adds the derivation the record
// layer needs, and the legacy six-syllable surface.

use sha2::{Digest, Sha256};
use tagma_core::{Coord, CoordPath};

use crate::fih::FihHash;

/// The FIH identity: the tagma id, with the record layer's depth as default.
pub type CoordId<const N: usize = COORD_ID_DEPTH> = tagma_core::CoordId<N>;

// The record layer holds the value and converts at its edges. Keying a map or an index by the text made every
// lookup pay a parse and a render; keyed by the value it pays neither. Measured at depth 20: about 55 ns to
// render, 35 ns to parse, against a lookup that needs neither.

/// Tagma coordinate path depth for FIH storage addressing.
/// Default: 20 → 11,172^20 ≈ 2^269 address space, the minimum depth that
/// injectively encodes a full 256-bit SHA-256 (19 × log2(11172) ≈ 255.5
/// < 256 would not).
pub const COORD_ID_DEPTH: usize = 20;

/// Encode a 256-bit digest as 20 base-11172 coordinates, most significant
/// first. A 256-bit value fits in 20 digits by construction.
pub fn encode_hash_into_coords(digest: &[u8; 32]) -> [Coord; 20] {
    base11172::digits_be::<20, 32>(digest).expect("a 256-bit digest fits in 20 digits")
}

/// The content hash of an identity: SHA-256 over its coordinate indices.
pub trait FihId {
    fn to_content_hash(&self) -> FihHash;
}

impl<const N: usize> FihId for CoordId<N> {
    fn to_content_hash(&self) -> FihHash {
        let mut h = Sha256::new();
        for coord in self.coords() {
            h.update(coord.index().to_le_bytes());
        }
        FihHash(h.finalize().into())
    }
}

/// Derivation of a full-depth identity from content or from a label.
pub trait FihId20: Sized {
    fn content_id(entity: u16, origin: &str, creator: &str, content_hash: &FihHash) -> Self;
    fn content_fact_id(origin: &str, creator: &str, content_hash: &FihHash) -> Self;
    fn from_label(label: &str) -> Self;
    fn resolve(s: &str) -> Self;
}

impl FihId20 for CoordId {
    /// The id is SHA-256 over (content_hash ‖ entity ‖ origin ‖ creator),
    /// encoded injectively into 20 coordinates, so distinct inputs map to
    /// distinct ids up to SHA-256 collision resistance.
    fn content_id(entity: u16, origin: &str, creator: &str, content_hash: &FihHash) -> Self {
        let mut h = Sha256::new();
        h.update(content_hash.0);
        h.update(entity.to_le_bytes());
        h.update(origin.as_bytes());
        h.update(creator.as_bytes());
        let digest: [u8; 32] = h.finalize().into();
        tagma_core::CoordId(CoordPath::new(encode_hash_into_coords(&digest)))
    }

    fn content_fact_id(origin: &str, creator: &str, content_hash: &FihHash) -> Self {
        Self::content_id(0, origin, creator, content_hash)
    }

    /// A stable id from an arbitrary label, through the semantic layer with
    /// the fixed origin ("label") and creator ("fixture").
    fn from_label(label: &str) -> Self {
        let hash = FihHash(Sha256::digest(label.as_bytes()).into());
        Self::content_id(0, "label", "fixture", &hash)
    }

    /// A 20-character canonical id passes through; any other text is a label
    /// and is derived. Resolution never fails, so a malformed canonical
    /// reference silently becomes a label id for a different record.
    fn resolve(s: &str) -> Self {
        CoordId::from_string(s).unwrap_or_else(|| Self::from_label(s))
    }
}

/// The legacy six-syllable surface. It is not used by the record layer and
/// carries no content-injective guarantee.
pub trait FihId6: Sized {
    fn new(counter: u64) -> Self;
    fn from_indices(indices: [u16; 6]) -> Option<Self>;
    fn axis(&self, idx: usize) -> Coord;
    fn from_axes(
        time_hi: u16,
        time_lo: u16,
        entity: u16,
        origin: u16,
        creator: u16,
        serial: u16,
    ) -> Option<Self>;
    fn with_timestamp(ts_ns: u64, entity: u16, origin: u16, creator: u16, serial: u16) -> Self;
    fn time_hi(&self) -> u16;
    fn entity_type(&self) -> u16;
}

impl FihId6 for CoordId<6> {
    fn new(counter: u64) -> Self {
        let mut remaining = counter;
        let mut coords = [Coord::new(0).unwrap(); 6];
        for c in coords.iter_mut() {
            let idx = (remaining % 11172) as u16;
            *c = Coord::new(idx).expect("coord index in 0..11172");
            remaining /= 11172;
        }
        tagma_core::CoordId(CoordPath::new(coords))
    }

    fn from_indices(indices: [u16; 6]) -> Option<Self> {
        let mut coords = [Coord::new(0).unwrap(); 6];
        for (i, &idx) in indices.iter().enumerate() {
            coords[i] = Coord::new(idx)?;
        }
        Some(tagma_core::CoordId(CoordPath::new(coords)))
    }

    // Axis: [0]time_hi [1]time_lo [2]entity [3]origin [4]creator [5]serial
    fn axis(&self, idx: usize) -> Coord {
        self.coords()[idx]
    }

    fn from_axes(
        time_hi: u16,
        time_lo: u16,
        entity: u16,
        origin: u16,
        creator: u16,
        serial: u16,
    ) -> Option<Self> {
        let coords = [
            Coord::new(time_hi % 11172)?,
            Coord::new(time_lo % 11172)?,
            Coord::new(entity % 11172)?,
            Coord::new(origin % 11172)?,
            Coord::new(creator % 11172)?,
            Coord::new(serial % 11172)?,
        ];
        Some(tagma_core::CoordId(CoordPath::new(coords)))
    }

    fn with_timestamp(ts_ns: u64, entity: u16, origin: u16, creator: u16, serial: u16) -> Self {
        let days = (ts_ns / 86_400_000_000_000) as u16;
        let sub = (ts_ns % 86_400_000_000_000) as u16;
        Self::from_axes(days, sub, entity, origin, creator, serial).unwrap()
    }

    fn time_hi(&self) -> u16 {
        self.axis(0).index()
    }

    fn entity_type(&self) -> u16 {
        self.axis(2).index()
    }
}
