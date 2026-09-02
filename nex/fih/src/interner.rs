use alloc::rc::Rc;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::cell::RefCell;

// `std::collections::HashMap` exists only under the std feature; alloc has
// no HashMap. The no_std path substitutes a BTreeMap, which satisfies the
// same contract (iteration order is unspecified for HashMap, so callers
// cannot rely on it).
#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap as HashMap;
#[cfg(feature = "std")]
use std::collections::HashMap;

/// Simple string interner — converts strings to `Rc<str>` for O(1) comparison
/// and reduced heap allocation on repeated strings (origin URLs, agent names).
///
/// FIH fields like `Fact::origin` and `Fact::creator` often repeat across many facts.
/// Using interned strings avoids `String` allocation per fact for common values.
///
/// Deprecated: Superseded by `FihCoord`'s internal `StringInterner` (u32-based)
/// in `nex/src/storage/core/index.rs`. This type is no longer used anywhere in
/// the codebase and will be removed in a future cleanup pass.
#[deprecated(note = "use FihCoord's internal StringInterner (u32-based) instead")]
pub struct Interner {
    to_id: RefCell<HashMap<Rc<str>, u32>>,
    to_str: RefCell<Vec<Rc<str>>>,
}

#[allow(deprecated)]
impl Interner {
    pub fn new() -> Self {
        Self {
            to_id: RefCell::new(HashMap::new()),
            to_str: RefCell::new(Vec::new()),
        }
    }

    pub fn intern(&self, s: &str) -> Rc<str> {
        if let Some(id) = self.to_id.borrow().get(s) {
            return self.to_str.borrow()[*id as usize].clone();
        }
        let rc: Rc<str> = Rc::from(s.to_string());
        let id = self.to_str.borrow().len() as u32;
        self.to_id.borrow_mut().insert(rc.clone(), id);
        self.to_str.borrow_mut().push(rc.clone());
        rc
    }

    pub fn count(&self) -> usize {
        self.to_str.borrow().len()
    }
}

#[allow(deprecated)]
impl Default for Interner {
    fn default() -> Self {
        Self::new()
    }
}
