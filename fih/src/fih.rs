use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::hash::{Hash, Hasher};
use tagma_core::{Coord, CoordPath};

// ── Tagma primary identity ─────────────────────────────────────────────

/// Tagma coordinate path depth for FIH storage addressing.
/// Default: 20 → 11,172^20 ≈ 2^269 address space, the minimum depth that
/// injectively encodes a full 256-bit SHA-256 (19 × log2(11172) ≈ 255.5
/// < 256 would not).
pub const COORD_ID_DEPTH: usize = 20;

/// A Tagma coordinate path used as the primary FIH identifier.
/// Depth defaults to `COORD_ID_DEPTH` (=20). Axis methods
/// (axis, from_axes, with_timestamp) are N=6-specific; the 6-syllable
/// form remains available as `CoordId<6>` for explicit small ids.
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

    /// Parse from string. Canonical-only: exactly N Hangul characters.
    /// Returns `None` for any other input. No hash fallback: id
    /// derivation belongs to the semantic layer (`CoordId::content_id`),
    /// and the string-to-path mapping contract is owned by chton.
    pub fn from_string(s: &str) -> Option<Self> {
        let chars: Vec<char> = s.chars().collect();
        if chars.len() == N && chars.iter().all(|c| Coord::from_char(*c).is_some()) {
            let mut coords = [Coord::new(0).unwrap(); N];
            for (i, &ch) in chars.iter().enumerate() {
                coords[i] = Coord::from_char(ch).unwrap();
            }
            Some(CoordId(CoordPath::new(coords)))
        } else {
            None
        }
    }

    /// Return the coordinate at the given path index.
    pub fn coord_at(&self, idx: usize) -> Coord {
        self.0.coords()[idx]
    }
}

/// Encode a 256-bit digest injectively into 20 base-11172 coordinates.
///
/// 20 × log2(11172) ≈ 268.9 ≥ 256, so every 32-byte value maps to a
/// unique coordinate sequence (big-endian base-11172 digits; the most
/// significant digit lands at coords[0]). Deterministic and
/// collision-free by construction (Step 4, #176).
fn encode_hash_into_coords(digest: &[u8; 32]) -> [Coord; 20] {
    let mut buf = *digest;
    let mut coords = [Coord::new(0).unwrap(); 20];
    let mut significant = 32usize;
    for coord in coords.iter_mut().rev() {
        let mut rem: u32 = 0;
        for byte in buf.iter_mut().take(significant) {
            let cur = (rem << 8) | *byte as u32;
            *byte = (cur / 11172) as u8;
            rem = cur % 11172;
        }
        *coord = Coord::new(rem as u16).expect("remainder < 11172");
        while significant > 0 && buf[significant - 1] == 0 {
            significant -= 1;
        }
    }
    coords
}

// ── Semantic id derivation ────────────────────────────────────────────

impl CoordId<20> {
    /// Full-injective content-addressed id (Step 4, #176).
    ///
    /// The id is SHA-256 over (content_hash ‖ entity ‖ origin ‖ creator),
    /// encoded injectively into 20 base-11172 coordinates (2^268.9 ≥
    /// 2^256), so distinct (content, context) inputs map to distinct ids
    /// up to SHA-256 collision resistance (2^128 birthday). The compact
    /// 6-syllable ~40-bit fold and its ~1.5M-record birthday ceiling are
    /// gone; the matching-map detection stays as defense-in-depth. The
    /// result is canonical (20 Hangul characters) and deterministic per
    /// (entity, origin, creator, content). The coords are opaque: the
    /// semantic axes no longer live in the id, ordering and filtering
    /// happen on record fields through the structural index.
    pub fn content_id(entity: u16, origin: &str, creator: &str, content_hash: &FihHash) -> Self {
        let mut h = Sha256::new();
        h.update(content_hash.0);
        h.update(entity.to_le_bytes());
        h.update(origin.as_bytes());
        h.update(creator.as_bytes());
        let digest: [u8; 32] = h.finalize().into();
        CoordId(CoordPath::new(encode_hash_into_coords(&digest)))
    }

    /// Content-addressed fact id: [`CoordId::content_id`] with the fact
    /// entity kind (0).
    pub fn content_fact_id(origin: &str, creator: &str, content_hash: &FihHash) -> Self {
        Self::content_id(0, origin, creator, content_hash)
    }

    /// Deterministic canonical id from an arbitrary label: the label is
    /// content-addressed through the semantic layer with the fixed
    /// origin ("label") and creator ("fixture"). Used by tests and
    /// external coordination where ids must be stable across runs and
    /// canonical at the store boundary.
    pub fn from_label(label: &str) -> Self {
        let hash = FihHash(Sha256::digest(label.as_bytes()).into());
        Self::content_id(0, "label", "fixture", &hash)
    }

    /// Resolve an id reference to a canonical id. Reference rules: a
    /// string of exactly 20 Hangul characters is a canonical id and
    /// passes through unchanged ([`CoordId::from_string`]); any other
    /// string is a label, derived through the semantic layer
    /// ([`CoordId::from_label`]). A label and its derived canonical form
    /// address the same record.
    ///
    /// The two namespaces are disjoint by length only: a 20-Hangul
    /// string is always canonical, never a label, so a label literally
    /// spelled as 20 Hangul characters cannot be addressed by that
    /// spelling, because `resolve` reads it as a canonical id.
    /// Resolution never fails: a malformed canonical reference silently
    /// becomes a label id for a different record. Strict callers that
    /// must reject non-canonical references should call
    /// [`CoordId::from_string`] and treat `None` as an error.
    pub fn resolve(s: &str) -> Self {
        Self::from_string(s).unwrap_or_else(|| Self::from_label(s))
    }
}

// ── N=6 specific methods (explicit CoordId<6> ids only) ──────────────

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
    pub fn time_hi(&self) -> u16 {
        self.axis(0).index()
    }
    /// Extract the entity type axis value.
    pub fn entity_type(&self) -> u16 {
        self.axis(2).index()
    }
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

// Display/Serialize/Deserialize for the default depth (20). Kept as
// explicit impls per depth (generic serde impls cause type inference
// failures with serde derive).

impl std::fmt::Display for CoordId<20> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for coord in self.0.iter() {
            write!(f, "{}", coord.to_char())?;
        }
        Ok(())
    }
}

impl Serialize for CoordId<20> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for CoordId<20> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s: String = Deserialize::deserialize(d)?;
        let chars: Vec<char> = s.chars().collect();
        if chars.len() != 20 {
            return Err(serde::de::Error::custom(format!(
                "CoordId<20> deserialize: expected 20 chars, got {}",
                chars.len()
            )));
        }
        let mut coords = [Coord::new(0).unwrap(); 20];
        for (i, &ch) in chars.iter().enumerate() {
            coords[i] = Coord::from_code_point(ch as u16).ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "CoordId<20> deserialize: char '{ch}' is not a valid coordinate"
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
/// `id` is a Tagma CoordPath (20 syllables, full-injective SHA-256
/// encoding since Step 4, #176) — the primary storage address.
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
    /// Create a content-addressed Fact.
    ///
    /// The id comes from the semantic layer (`CoordId::content_fact_id`):
    /// deterministic per content + origin + creator, canonical 20-Hangul,
    /// injectively encoding the full SHA-256.
    /// `content_hash` is computed simultaneously so only one SHA-256
    /// pass is needed.
    pub fn new(origin: String, content: Content, creator: String) -> Self {
        let content_hash = {
            let Content { data, .. } = &content;
            let mut h = Sha256::new();
            h.update(data);
            FihHash(h.finalize().into())
        };
        let id = CoordId::content_fact_id(&origin, &creator, &content_hash);
        Fact {
            id,
            content_hash,
            origin,
            content,
            creator,
        }
    }

    /// Create a Fact with an explicit CoordId (opt out of content-addressed ID).
    /// Use when the caller needs a specific ID (e.g., for pre-coordinated references
    /// in tests, or when ID format is dictated by external protocol).
    pub fn with_id(id: CoordId, origin: String, content: Content, creator: String) -> Self {
        let content_hash = {
            let Content { data, .. } = &content;
            let mut h = Sha256::new();
            h.update(data);
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
