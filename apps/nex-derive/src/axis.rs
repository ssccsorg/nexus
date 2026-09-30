// The three axes and the coordinate packing.
//
// A goal is a point in the product of the three axes: work is the domain the
// goal belongs to, intent is its direction, and hint is the contract that
// bounds it. A derivation records the goal that produced a fact, so the board
// is a set of goals, and resolving a conjunctive goal is an intersection over
// them.

use core::fmt;

use crate::layout::{AXES, AXIS_BITS, AXIS_CARD};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    /// The domain of work the goal belongs to.
    Work,
    /// The direction of the goal.
    Intent,
    /// The contract that bounds the goal.
    Hint,
}

impl Axis {
    pub const ALL: [Axis; AXES] = [Axis::Work, Axis::Intent, Axis::Hint];

    pub fn index(self) -> usize {
        match self {
            Axis::Work => 0,
            Axis::Intent => 1,
            Axis::Hint => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Axis::Work => "work",
            Axis::Intent => "intent",
            Axis::Hint => "hint",
        }
    }

    pub fn parse(text: &str) -> Option<Axis> {
        match text {
            "work" | "w" | "fact" | "f" => Some(Axis::Work),
            "intent" | "i" => Some(Axis::Intent),
            "hint" | "h" | "contract" | "c" => Some(Axis::Hint),
            _ => None,
        }
    }
}

/// Packs axis values into a coordinate index. `values` is in `Axis::ALL` order.
pub fn pack(values: [u8; AXES]) -> usize {
    debug_assert!(values.iter().all(|v| (*v as usize) < AXIS_CARD));
    let mut coord = 0usize;
    for value in values {
        coord = (coord << AXIS_BITS) | (value as usize);
    }
    coord
}

/// Unpacks a coordinate index back into axis values, in `Axis::ALL` order.
pub fn unpack(coord: usize) -> [u8; AXES] {
    let mut values = [0u8; AXES];
    let mut rest = coord;
    for slot in values.iter_mut().rev() {
        *slot = (rest & (AXIS_CARD - 1)) as u8;
        rest >>= AXIS_BITS;
    }
    values
}

/// Symbolic names for axis values.
///
/// Indices are assigned in registration order, so a name is a stable address
/// on its axis once it is defined. The CLI interns names on `place` so a
/// transcript stays readable; `define` pins them ahead of use when the
/// insertion order should not depend on the order records are placed.
#[derive(Debug, Default)]
pub struct Names {
    values: [Vec<String>; AXES],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    /// The axis already holds `AXIS_CARD` values.
    Full(Axis),
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NameError::Full(axis) => {
                write!(f, "{} axis is full ({} values)", axis.name(), AXIS_CARD)
            }
        }
    }
}

impl Names {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the index of `name` on `axis`, registering it when it is new.
    pub fn intern(&mut self, axis: Axis, name: &str) -> Result<u8, NameError> {
        let slot = &mut self.values[axis.index()];
        if let Some(existing) = slot.iter().position(|candidate| candidate == name) {
            return Ok(existing as u8);
        }
        if slot.len() >= AXIS_CARD {
            return Err(NameError::Full(axis));
        }
        slot.push(name.to_string());
        Ok((slot.len() - 1) as u8)
    }

    /// Returns the index of an already-defined `name`, or `None`.
    pub fn lookup(&self, axis: Axis, name: &str) -> Option<u8> {
        self.values[axis.index()]
            .iter()
            .position(|candidate| candidate == name)
            .map(|index| index as u8)
    }

    /// Returns the name at `value`, or `None` when that index is unnamed.
    pub fn label(&self, axis: Axis, value: u8) -> Option<&str> {
        self.values[axis.index()]
            .get(value as usize)
            .map(String::as_str)
    }

    pub fn list(&self, axis: Axis) -> &[String] {
        &self.values[axis.index()]
    }
}
