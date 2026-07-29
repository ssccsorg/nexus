use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::hash::{Hash, Hasher};
use tagma_core::{Coord, CoordPath};

// ── Tagma primary identity ─────────────────────────────────────────────

/// Tagma coordinate path depth for FIH storage addressing.
/// Default: 6 → 11,172^6 ≈ 2×10^24 address space.
/// Use `CoordId<20>` for SHA-256-scale space.
pub const COORD_ID_DEPTH: usize = 6;

/// A Tagma coordinate path used as the primary FIH identifier.
/// Depth defaults to `COORD_ID_DEPTH` (=6). Use `CoordId<20>` when needed.
/// Axis methods (axis, from_axes, with_timestamp) are N=6-specific.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordId<const N: usize = COORD_ID_DEPTH>(pub CoordPath<N>);

impl<const N: usize> Hash for CoordId<N> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for coord in self.0.iter() {
            coord.index().hash(state);
        }
    }
}

// ── Generic methods (works for any depth N) ─────────────────────────

impl<const N: usize> CoordId<N> {
    /// Derive a SHA-256 content hash from coord path indices.
    pub fn to_content_hash(&self) -> FihHash {
        let mut h = Sha256::new();
        for coord in self.0.iter() {
            h.update(coord.index().to_le_bytes());
        }
        FihHash(h.finalize().into())
    }

    /// Parse from string. If N Hangul chars, maps directly. Otherwise hashes.
    pub fn from_string(s: &str) -> Self {
        let chars: Vec<char> = s.chars().collect();
        if chars.len() == N && chars.iter().all(|c| Coord::from_char(*c).is_some()) {
            let mut coords = [Coord::new(0).unwrap(); N];
            for (i, &ch) in chars.iter().enumerate() {
                coords[i] = Coord::from_char(ch).unwrap();
            }
            CoordId(CoordPath::new(coords))
        } else {
            let mut h = Sha256::new();
            h.update(s.as_bytes());
            let hash: [u8; 32] = h.finalize().into();
            let mut coords = [Coord::new(0).unwrap(); N];
            for (i, coord) in coords.iter_mut().enumerate() {
                let idx = u16::from_le_bytes([
                    hash.get(i * 2).copied().unwrap_or(0),
                    hash.get(i * 2 + 1).copied().unwrap_or(0),
                ]) % 11172;
                *coord = Coord::new(idx).unwrap();
            }
            CoordId(CoordPath::new(coords))
        }
    }

    /// Return the coordinate at the given path index.
    pub fn coord_at(&self, idx: usize) -> Coord {
        self.0.coords()[idx]
    }
}

// ── N=6 specific methods (default depth) ──────────────────────────────

impl CoordId<6> {
    /// Generate from a 64-bit counter (~1.94e24 unique sequential IDs).
    pub fn new(counter: u64) -> Self {
        let mut remaining = counter;
        let mut coords = [Coord::new(0).unwrap(); 6];
        for c in coords.iter_mut() {
            let idx = (remaining % 11172) as u16;
            *c = Coord::new(idx).expect("coord index in 0..11172");
            remaining /= 11172;
        }
        CoordId(CoordPath::new(coords))
    }

    /// Generate a CoordId from raw 6 coord indices (0..11172 each).
    pub fn from_indices(indices: [u16; 6]) -> Option<Self> {
        let mut coords = [Coord::new(0).unwrap(); 6];
        for (i, &idx) in indices.iter().enumerate() {
            coords[i] = Coord::new(idx)?;
        }
        Some(CoordId(CoordPath::new(coords)))
    }

    // ── Axis accessors (CoordPath<6> convention) ────────────────
    // Axis: [0]time_hi [1]time_lo [2]entity [3]origin [4]creator [5]serial
    //   [0]: time_hi   — coarse time bucket (epoch day)
    //   [1]: time_lo   — fine time (sequence within bucket)
    //   [2]: entity    — entity type (0=Fact, 1=Intent, 2=Hint)
    //   [3]: origin    — origin category
    //   [4]: creator   — creator category
    //   [5]: serial    — uniqueness discriminator

    /// Return the coordinate at the given semantic axis (0..5).
    pub fn axis(&self, idx: usize) -> Coord {
        self.0.coords()[idx]
    }

    /// Build a CoordId with explicit axis values.
    pub fn from_axes(
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
        Some(CoordId(CoordPath::new(coords)))
    }

    /// Create a CoordId with time_hi/time_lo set from a nanosecond timestamp.
    pub fn with_timestamp(ts_ns: u64, entity: u16, origin: u16, creator: u16, serial: u16) -> Self {
        let days = (ts_ns / 86_400_000_000_000) as u16;
        let sub = (ts_ns % 86_400_000_000_000) as u16;
        Self::from_axes(days, sub, entity, origin, creator, serial).unwrap()
    }

    /// Extract the time_hi axis value (days since epoch).
    pub fn time_hi(&self) -> u16 { self.axis(0).index() }
    /// Extract the entity type axis value.
    pub fn entity_type(&self) -> u16 { self.axis(2).index() }
}

// Display/Serialize/Deserialize are N=6 specific. Generic versions
// cause type inference failures with serde derive. Users of CoordId<20>
// must implement these manually for their depth.

impl std::fmt::Display for CoordId<6> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for coord in self.0.iter() {
            write!(f, "{}", coord.to_char())?;
        }
        Ok(())
    }
}

impl Serialize for CoordId<6> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for CoordId<6> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s: String = Deserialize::deserialize(d)?;
        let chars: Vec<char> = s.chars().collect();
        if chars.len() != 6 {
            return Err(serde::de::Error::custom(format!(
                "CoordId<6> deserialize: expected 6 chars, got {}",
                chars.len()
            )));
        }
        let mut coords = [Coord::new(0).unwrap(); 6];
        for (i, &ch) in chars.iter().enumerate() {
            coords[i] = Coord::from_code_point(ch as u16).ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "CoordId<6> deserialize: char '{ch}' is not a valid coordinate"
                ))
            })?;
        }
        Ok(CoordId(CoordPath::new(coords)))
    }
}

// ── Content-addressable identifier (demoted to content integrity only) ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FihHash(pub [u8; 32]);

impl Serialize for FihHash {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for FihHash {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let hex: String = Deserialize::deserialize(d)?;
        if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(serde::de::Error::custom(format!(
                "FihHash deserialize: expected 64 hex chars, got {}",
                hex.len()
            )));
        }
        let mut bytes = [0u8; 32];
        for i in 0..32 {
            bytes[i] = u8::from_str_radix(&hex[i * 2..=i * 2 + 1], 16)
                .map_err(|e| serde::de::Error::custom(format!("invalid hex: {e}")))?;
        }
        Ok(Self(bytes))
    }
}

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

    fn parse_hex_strict(hex: &str) -> Self {
        assert!(
            hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()),
            "FihHash::parse_hex_strict: expected 64 hex chars, got `{}`",
            hex
        );
        let mut bytes = [0u8; 32];
        for i in 0..32 {
            bytes[i] = u8::from_str_radix(&hex[i * 2..=i * 2 + 1], 16).expect("valid hex digit");
        }
        Self(bytes)
    }

    pub fn from_hex(hex: &str) -> Self {
        let hex_clean: String = hex.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        if hex_clean.len() == 64 {
            Self::parse_hex_strict(&hex_clean)
        } else {
            let mut h = Sha256::new();
            h.update(hex.as_bytes());
            Self(h.finalize().into())
        }
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

/// Fact: an immutable state snapshot.
/// `id` is a Tagma CoordPath (6 syllables) — the primary storage address.
/// `content_hash` is SHA-256 of content, used for blob dedup and integrity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub id: CoordId,
    pub content_hash: FihHash,
    pub origin: String,
    pub content: Content,
    pub creator: String,
}

impl Fact {
    pub fn new(id: CoordId, origin: String, content: Content, creator: String) -> Self {
        let content_hash = {
            let Content { data, .. } = &content;
            let bytes = data;
            let mut h = Sha256::new();
            h.update(bytes);
            FihHash(h.finalize().into())
        };
        Fact {
            id,
            content_hash,
            origin,
            content,
            creator,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub id: CoordId,
    pub from_facts: Vec<CoordId>,
    pub to_fact_id: Option<CoordId>,
    pub description: String,
    pub creator: String,
    pub worker: Option<String>,
    pub last_heartbeat_at: Option<u64>,
    pub created_at: Option<u64>,
    pub is_concluded: bool,
    pub concluded_at: Option<u64>,
}

impl Intent {
    pub fn new(
        id: CoordId,
        from_facts: Vec<CoordId>,
        to_fact_id: Option<CoordId>,
        description: String,
        creator: String,
    ) -> Self {
        Intent {
            id,
            from_facts,
            to_fact_id,
            description,
            creator,
            worker: None,
            last_heartbeat_at: None,
            created_at: None,
            is_concluded: false,
            concluded_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hint {
    pub id: CoordId,
    pub content: String,
    pub creator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardState {
    pub facts: Vec<Fact>,
    pub intents: Vec<Intent>,
    pub hints: Vec<Hint>,
}
