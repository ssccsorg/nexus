// nexus-graph — Weight types for petgraph storage.

use nexus_model::Content;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeWeight {
    pub name: String,
    pub label: String,
    pub properties: HashMap<String, Content>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeWeight {
    pub rel_type: String,
    pub properties: HashMap<String, Content>,
}
