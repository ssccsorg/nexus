// ── nex-router: which node answers which method ────────────────────────
//
// The hub holds no records. It holds this: a map from a method to the node that answers it,
// built from what each node registers. Routing is therefore a lookup, and the table is the
// whole of the hub's state, which is why it fits a node with no memory to spare.
//
// A method has one owner. A second node claiming it is refused rather than silently
// replacing the first, because a client that reaches a different node for the same method
// than it did a moment ago is a bug the hub would be hiding.

#![no_std]
extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// The hub's routing table.
#[derive(Default)]
pub struct Router {
    by_method: BTreeMap<String, String>,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    /// Make `node` the owner of every method it names.
    ///
    /// Re-registering the same node for the same method is idempotent, so a node that
    /// reconnects after a dropped link can register again. A different node claiming a
    /// method that already has an owner is refused.
    pub fn register(&mut self, node: &str, methods: &[String]) -> Result<(), String> {
        for method in methods {
            match self.by_method.get(method) {
                Some(owner) if owner == node => {}
                Some(owner) => {
                    return Err(alloc::format!(
                        "{method} is answered by {owner}, so {node} cannot claim it"
                    ));
                }
                None => {
                    self.by_method.insert(method.clone(), node.to_string());
                }
            }
        }
        Ok(())
    }

    /// Drop every method this node owned.
    pub fn deregister(&mut self, node: &str) {
        self.by_method.retain(|_, owner| owner != node);
    }

    /// The node that answers `method`.
    pub fn route(&self, method: &str) -> Option<&str> {
        self.by_method.get(method).map(|owner| owner.as_str())
    }

    /// The nodes the table knows, in name order.
    pub fn nodes(&self) -> Vec<String> {
        let mut nodes: Vec<String> = self.by_method.values().cloned().collect();
        nodes.sort_unstable();
        nodes.dedup();
        nodes
    }

    /// The methods one node owns, in name order.
    pub fn methods_of(&self, node: &str) -> Vec<String> {
        self.by_method
            .iter()
            .filter(|(_, owner)| owner.as_str() == node)
            .map(|(method, _)| method.clone())
            .collect()
    }

    /// How many methods have an owner.
    pub fn len(&self) -> usize {
        self.by_method.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_method.is_empty()
    }
}
