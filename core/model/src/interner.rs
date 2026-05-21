use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Simple string interner — converts strings to `Rc<str>` for O(1) comparison
/// and reduced heap allocation on repeated strings (origin URLs, agent names).
///
/// FIH fields like `Fact::origin` and `Fact::creator` often repeat across many facts.
/// Using interned strings avoids `String` allocation per fact for common values.
pub struct Interner {
    to_id: RefCell<HashMap<Rc<str>, u32>>,
    to_str: RefCell<Vec<Rc<str>>>,
}

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

impl Default for Interner {
    fn default() -> Self {
        Self::new()
    }
}
