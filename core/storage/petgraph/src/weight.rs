// nexus-graph — Weight types for petgraph storage.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod serde_properties {
    use serde::{Deserialize, Deserializer, Serializer};
    use serde_json::Value;
    use std::collections::HashMap;

    pub fn serialize<S>(map: &HashMap<String, Value>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let json_str = serde_json::to_string(map).map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&json_str)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<String, Value>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let json_str = String::deserialize(deserializer)?;
        serde_json::from_str(&json_str).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeWeight {
    pub name: String,
    pub label: String,
    #[serde(with = "serde_properties")]
    pub properties: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeWeight {
    pub rel_type: String,
    #[serde(with = "serde_properties")]
    pub properties: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub fields: HashMap<String, serde_json::Value>,
}
