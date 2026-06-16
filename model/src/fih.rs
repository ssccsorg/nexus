use serde::{Deserialize, Serialize};

/// Returns true if the character is a valid hex digit (0-9, a-f, A-F).
fn is_hex_char(c: char) -> bool {
    c.is_ascii_hexdigit()
}
use sha2::{Digest, Sha256};

// ── Content-addressable identifier ───────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FihHash(pub [u8; 32]);

impl std::fmt::Display for FihHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in &self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

impl FihHash {
    pub fn new(fields: &[&str], type_tag: &str) -> Self {
        let mut h = Sha256::new();
        for f in fields {
            h.update(f.as_bytes());
        }
        h.update(type_tag.as_bytes());
        Self(h.finalize().into())
    }

    pub fn chain(a: &FihHash, b: &FihHash, c: &FihHash) -> FihHash {
        let mut h = Sha256::new();
        h.update(a.0);
        h.update(b.0);
        h.update(c.0);
        Self(h.finalize().into())
    }
}

/// Allow constructing FihHash from a hex string literal in tests.
impl FihHash {
    /// Create from a hex string (64 hex chars = 32 bytes).
    /// If the string is shorter than 64 chars, it's left-padded with zeros.
    /// Non-hex characters are filtered out.
    pub fn from_hex(hex: &str) -> Self {
        let mut bytes = [0u8; 32];
        let hex_clean: String = hex.chars().filter(|c| is_hex_char(*c)).collect();
        let start = if hex_clean.len() > 64 {
            hex_clean.len() - 64
        } else {
            0
        };
        let relevant = &hex_clean[start..];
        let padding = 64 - relevant.len().min(64);
        for i in 0..relevant.len().min(64) / 2 {
            if let Ok(v) = u8::from_str_radix(&relevant[i * 2..=i * 2 + 1], 16) {
                bytes[(padding / 2) + i] = v;
            }
        }
        Self(bytes)
    }
}

impl From<&str> for FihHash {
    fn from(s: &str) -> Self {
        Self::from_hex(s)
    }
}

// ── Content ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Content {
    pub mime_type: String,
    pub data: Vec<u8>,
}

impl Content {
    pub fn as_str(&self) -> Option<&str> {
        match self.mime_type.as_str() {
            "text/plain" | "application/json" => std::str::from_utf8(&self.data).ok(),
            _ => None,
        }
    }
}

impl std::fmt::Display for Content {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.mime_type.as_str() {
            "text/plain" | "application/json" => {
                if let Ok(s) = std::str::from_utf8(&self.data) {
                    write!(f, "{s}")
                } else {
                    write!(f, "<invalid utf-8 for {}>", self.mime_type)
                }
            }
            _ => write!(f, "<{}: {} bytes>", self.mime_type, self.data.len()),
        }
    }
}

impl From<String> for Content {
    fn from(s: String) -> Self {
        Content {
            mime_type: "text/plain".into(),
            data: s.into_bytes(),
        }
    }
}

impl From<&str> for Content {
    fn from(s: &str) -> Self {
        Content {
            mime_type: "text/plain".into(),
            data: s.as_bytes().to_vec(),
        }
    }
}

impl PartialEq<&str> for Content {
    fn eq(&self, other: &&str) -> bool {
        self.mime_type == "text/plain" && self.data.as_slice() == other.as_bytes()
    }
}

// ── FIH Primitives ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub id: FihHash,
    pub origin: String,
    pub content: Content,
    pub creator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub id: FihHash,
    pub from_facts: Vec<FihHash>,
    pub to_fact_id: Option<FihHash>,
    pub description: String,
    pub creator: String,
    pub worker: Option<String>,
    pub last_heartbeat_at: Option<u64>,
    pub created_at: Option<u64>,
    pub is_concluded: bool,
    pub concluded_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hint {
    pub id: FihHash,
    pub content: String,
    pub creator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardState {
    pub facts: Vec<Fact>,
    pub intents: Vec<Intent>,
    pub hints: Vec<Hint>,
}
