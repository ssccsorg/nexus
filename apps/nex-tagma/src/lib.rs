pub use tagma_core::TagmaCoord as Coord;

/// Convenience: check whether a Unicode code point is a valid Tagma coordinate.
pub fn validate(cp: u16) -> bool {
    Coord::from_code_point(cp).is_some()
}
