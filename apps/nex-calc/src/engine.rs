// Calculator engine — FIH-based computation via FihStorage<SimIo>.
//
//   F (Fact)  = number stored as immutable, content-addressed Fact
//   I (Intent) = operator with direction through the FIH state space
//   H (Hint)   = constraint or transform on computation
//
// Storage is backed by FihStorage<SimIo>: the same storage engine used
// throughout the neXus ecosystem. The IO layer (SimIo) is in-memory.
// Swapping SimIo for FsIo or CfFihIo changes the persistence layer
// without touching calculator logic — FihStorage's IO abstraction.
//
// Number Facts store their value as a blob via the IO layer:
//   blob/{blob_hash}.bin          ← i64 little-endian bytes
//   blob/{blob_hash}.bin.meta     ← ContentMeta (mime type, size)
//
// The FihHash is content-addressed: SHA256(value_string + tag).

use std::fmt;

use sha2::{Digest, Sha256};

use nex::FileIo;
use nex::storage::core::intent_status::IntentStatus;
use nex::storage::core::record::ContentMeta;
use nex::storage::core::store::{FihStorage, Record};
use nex_fih::{Content, CoordId, FihHash};
use nexus_storage_sim::SimIo;

use crate::hint::Constraint;
use crate::ops::OpType;

const NUMBER_MIME: &str = "application/x-nex-calc-number";
/// Coord path depth for the unified store (must match STORE_DEPTH in store.rs).
const STORE_DEPTH: usize = 19;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalcError {
    FactNotFound(String),
    IntentNotFound(String),
    OpError(String),
    ConstraintViolated {
        hint_id: String,
        constraint: String,
        result: i64,
    },
    AlreadyResolved(String),
}

impl fmt::Display for CalcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CalcError::FactNotFound(id) => write!(f, "fact not found: {id}"),
            CalcError::IntentNotFound(id) => write!(f, "intent not found: {id}"),
            CalcError::OpError(msg) => write!(f, "operator error: {msg}"),
            CalcError::ConstraintViolated {
                hint_id,
                constraint,
                result,
            } => {
                write!(
                    f,
                    "constraint violated [{hint_id}]: {constraint} (got {result})"
                )
            }
            CalcError::AlreadyResolved(id) => write!(f, "intent already resolved: {id}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedIntent {
    pub intent_id: CoordId,
    pub op: OpType,
    pub lhs: i64,
    pub rhs: i64,
    pub result_id: CoordId,
    pub result_value: i64,
}

/// Calculator engine backed by FihStorage<SimIo>.
///
/// All state lives in FihStorage's in-memory unified store. The IO layer
/// (SimIo) handles content blob reads and writes — entirely in memory,
/// no filesystem. Calculator logic only sees the FihStorage API.
pub struct CalcEngine {
    storage: FihStorage<SimIo>,
}

impl CalcEngine {
    pub fn new() -> Self {
        Self {
            storage: FihStorage::new(SimIo::new(), "nex-calc"),
        }
    }

    // ── Fact operations ───────────────────────────────────────────

    /// Store a number as a Fact. Content-addressed via SHA256 of the value.
    pub async fn put(&self, value: i64) -> CoordId {
        let id = make_number_fact_id(value);
        let id_str = id.to_string();
        if self.storage.fact_exists(&id_str) {
            return id;
        }

        let data = value.to_le_bytes().to_vec();
        let blob_hash = content_hash(&data);
        let blob_path = format!("blob/{}.bin", blob_hash);

        // Write content blob and metadata via the IO layer.
        let _ = self.storage.io.write(&blob_path, &data).await;
        write_blob_meta(&self.storage.io, &blob_hash, NUMBER_MIME, data.len()).await;

        // Build content hash from the value string (consistent with make_number_fact_id).
        let content_hash = FihHash::new(&[&value.to_string()], "nex-calc");

        let record = Record::Fact {
            content: Content {
                data,
                mime_type: NUMBER_MIME.into(),
            },
            content_hash,
            origin: "nex-calc".into(),
            creator: "user".into(),
            submitted_at: 0,
        };

        let path = make_record_path(0u16, "nex-calc", "user", 0u16, &id_str, 0);
        self.storage.store.borrow_mut().place_path(&path, record);
        id
    }

    /// Read a number from a Fact.
    pub async fn get(&self, fact_id: &CoordId) -> Option<i64> {
        let (content, _content_hash, _origin, _creator) =
            self.storage.get_fact_by_id(&fact_id.to_string())?;
        if content.data.len() != 8 {
            return None;
        }
        let mut arr = [0u8; 8];
        arr.copy_from_slice(&content.data);
        Some(i64::from_le_bytes(arr))
    }

    /// Look up a Fact by short hex prefix of its CoordId string.
    pub async fn find_fact(&self, prefix: &str) -> Option<CoordId> {
        let prefix_lower = prefix.to_lowercase();
        for id_str in self.storage.all_fact_ids() {
            if id_str.to_lowercase().starts_with(&prefix_lower) {
                return Some(CoordId::from_string(&id_str));
            }
        }
        None
    }

    // ── Intent operations ─────────────────────────────────────────

    /// Create an operator Intent. Returns its content-addressed CoordId.
    pub async fn op(
        &self,
        op: OpType,
        lhs_id: &CoordId,
        rhs_id: &CoordId,
    ) -> Result<CoordId, CalcError> {
        if !self.storage.fact_exists(&lhs_id.to_string()) {
            return Err(CalcError::FactNotFound(lhs_id.to_string()));
        }
        if !self.storage.fact_exists(&rhs_id.to_string()) {
            return Err(CalcError::FactNotFound(rhs_id.to_string()));
        }

        let id = make_intent_id(op, lhs_id, rhs_id);
        let id_str = id.to_string();
        if self.storage.intent_exists(&id_str) {
            return Ok(id);
        }

        let now = nanos();
        let desc = format!("{}", op);

        let record = Record::Intent {
            from_facts: vec![lhs_id.to_string(), rhs_id.to_string()],
            description_hash: desc,
            creator: "user".into(),
            status: IntentStatus::Submitted,
            created_at: now,
        };

        let path = make_record_path(1u16, "", "user", 0u16, &id_str, now);
        self.storage.store.borrow_mut().place_path(&path, record);
        Ok(id)
    }

    /// Resolve an Intent — this IS the computation. Traverses the FIH space:
    ///
    ///   Fact(lhs) ─┐
    ///               ├── Intent(op) ──→ Fact(result)
    ///   Fact(rhs) ─┘        ↑
    ///                   Hint gates
    pub async fn resolve(&self, intent_id: &CoordId) -> Result<ResolvedIntent, CalcError> {
        let id_str = intent_id.to_string();
        let (from_facts, description_hash, _creator, status, created_at) = self
            .storage
            .get_intent_by_id(&id_str)
            .ok_or_else(|| CalcError::IntentNotFound(id_str.clone()))?;

        if matches!(status, IntentStatus::Concluded { .. }) {
            return Err(CalcError::AlreadyResolved(id_str));
        }

        let op = OpType::parse(&description_hash).ok_or_else(|| {
            CalcError::OpError(format!(
                "unknown operator '{}' in intent {}",
                description_hash, id_str
            ))
        })?;

        let lhs_fid = from_facts
            .first()
            .ok_or_else(|| CalcError::IntentNotFound("missing lhs".into()))?;
        let rhs_fid = from_facts
            .get(1)
            .ok_or_else(|| CalcError::IntentNotFound("missing rhs".into()))?;

        let lhs = self
            .get(&CoordId::from_string(lhs_fid))
            .await
            .ok_or_else(|| CalcError::FactNotFound(lhs_fid.clone()))?;
        let rhs = self
            .get(&CoordId::from_string(rhs_fid))
            .await
            .ok_or_else(|| CalcError::FactNotFound(rhs_fid.clone()))?;

        // Apply operand transforms, then operator.
        let (lhs, rhs) = self.apply_operand_transforms(lhs, rhs).await;
        let raw_result = op
            .apply(lhs, rhs)
            .map_err(|e| CalcError::OpError(e.to_string()))?;

        // Check result constraints.
        self.check_constraints(raw_result).await?;

        // Create the result Fact (content-addressed, so deduplicated).
        let result_id = make_number_fact_id(raw_result);
        let result_id_str = result_id.to_string();
        if !self.storage.fact_exists(&result_id_str) {
            let data = raw_result.to_le_bytes().to_vec();
            let bh = content_hash(&data);
            let _ = self
                .storage
                .io
                .write(&format!("blob/{}.bin", bh), &data)
                .await;
            write_blob_meta(&self.storage.io, &bh, NUMBER_MIME, data.len()).await;

            let content_hash = FihHash::new(&[&raw_result.to_string()], "nex-calc");
            let rec = Record::Fact {
                content: Content {
                    data,
                    mime_type: NUMBER_MIME.into(),
                },
                content_hash,
                origin: format!("nex-calc:resolve:{}", intent_id),
                creator: "nex-calc".into(),
                submitted_at: 0,
            };
            let path = make_record_path(
                0u16,
                &format!("nex-calc:resolve:{}", intent_id),
                "nex-calc",
                0u16,
                &result_id_str,
                0,
            );
            self.storage.store.borrow_mut().place_path(&path, rec);
        }

        // Mark intent concluded: vacate old path, insert at new path with Concluded status.
        let now = nanos();
        // Determine old status coord from the current status.
        let old_status_coord: u16 = match &status {
            IntentStatus::Submitted => 0,
            IntentStatus::Claimed { .. } => 1,
            IntentStatus::Concluded { .. } => 2,
        };
        let new_status = IntentStatus::Concluded {
            to_fact: result_id_str,
            concluded_at: now,
            worker: "nex-calc".into(),
        };
        // Remove old entry.
        let old_path = make_record_path(1u16, "", "user", old_status_coord, &id_str, created_at);
        self.storage.store.borrow_mut().vacate_path(&old_path);
        // Insert new entry.
        let new_path = make_record_path(1u16, "", "user", 2u16, &id_str, created_at);
        let new_record = Record::Intent {
            from_facts,
            description_hash,
            creator: "user".into(),
            status: new_status,
            created_at,
        };
        self.storage.store.borrow_mut().place_path(&new_path, new_record);

        Ok(ResolvedIntent {
            intent_id: *intent_id,
            op,
            lhs,
            rhs,
            result_id,
            result_value: raw_result,
        })
    }

    // ── Hint operations ───────────────────────────────────────────

    /// Add a constraint Hint.
    pub async fn constrain(&self, constraint: Constraint) -> CoordId {
        let id = make_hint_id(&constraint);
        let id_str = id.to_string();
        if !self.storage.hint_exists(&id_str) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let record = Record::Hint {
                content: constraint.to_string(),
                creator: "user".into(),
                submitted_at: now,
            };
            let path = make_record_path(2u16, "", "user", 0u16, &id_str, now * 1_000_000_000);
            self.storage.store.borrow_mut().place_path(&path, record);
        }
        id
    }

    pub async fn clear_hints(&self) {
        // Collect paths of all hint records, then remove them.
        let paths: Vec<_> = {
            let store = self.storage.store.borrow();
            store
                .iter_tree()
                .filter_map(|(path, record)| match record {
                    Record::Hint { .. } => Some(path),
                    _ => None,
                })
                .collect()
        };
        let mut store = self.storage.store.borrow_mut();
        for path in &paths {
            store.vacate_path(path);
        }
    }

    // ── Queries ───────────────────────────────────────────────────

    pub async fn list_facts(&self) -> Vec<(CoordId, i64)> {
        let ids = self.storage.all_fact_ids();
        let mut out = Vec::with_capacity(ids.len());
        for id_str in ids {
            if let Some((content, _, _, _)) = self.storage.get_fact_by_id(&id_str) {
                if content.data.len() == 8 {
                    let mut arr = [0u8; 8];
                    arr.copy_from_slice(&content.data);
                    out.push((CoordId::from_string(&id_str), i64::from_le_bytes(arr)));
                }
            }
        }
        out
    }

    pub async fn list_intents(&self) -> Vec<(CoordId, bool)> {
        let ids = self.storage.all_intent_ids();
        let mut out = Vec::with_capacity(ids.len());
        for id_str in ids {
            if let Some((_, _, _, status, _)) = self.storage.get_intent_by_id(&id_str) {
                out.push((
                    CoordId::from_string(&id_str),
                    matches!(status, IntentStatus::Concluded { .. }),
                ));
            }
        }
        out
    }

    pub async fn list_hints(&self) -> Vec<(CoordId, String)> {
        let ids = self.storage.all_hint_ids();
        let mut out = Vec::with_capacity(ids.len());
        for id_str in ids {
            if let Some((content, _)) = self.storage.get_hint_by_id(&id_str) {
                out.push((CoordId::from_string(&id_str), content));
            }
        }
        out
    }

    pub async fn fact_count(&self) -> usize {
        self.storage.all_fact_ids().len()
    }

    pub async fn pending_count(&self) -> usize {
        let ids = self.storage.all_intent_ids();
        let mut count = 0usize;
        for id_str in ids {
            if let Some((_, _, _, status, _)) = self.storage.get_intent_by_id(&id_str) {
                if !matches!(status, IntentStatus::Concluded { .. }) {
                    count += 1;
                }
            }
        }
        count
    }

    // ── Internal ──────────────────────────────────────────────────

    async fn apply_operand_transforms(&self, mut lhs: i64, mut rhs: i64) -> (i64, i64) {
        for id_str in self.storage.all_hint_ids() {
            if let Some((content, _)) = self.storage.get_hint_by_id(&id_str) {
                let c = match Constraint::parse_str(&content) {
                    Some(c) => c,
                    None => continue,
                };
                let (l, r2) = c.transform_operands(lhs, rhs);
                lhs = l;
                rhs = r2;
            }
        }
        (lhs, rhs)
    }

    async fn check_constraints(&self, result: i64) -> Result<(), CalcError> {
        for id_str in self.storage.all_hint_ids() {
            if let Some((content, _)) = self.storage.get_hint_by_id(&id_str) {
                let c = match Constraint::parse_str(&content) {
                    Some(c) => c,
                    None => continue,
                };
                if !c.check(result) {
                    return Err(CalcError::ConstraintViolated {
                        hint_id: id_str,
                        constraint: c.to_string(),
                        result,
                    });
                }
            }
        }
        Ok(())
    }
}

impl Default for CalcEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Build a CoordPath<19> matching the convention in store.rs:
///   [0] time_hi, [1] time_lo, [2] entity, [3] origin, [4] creator,
///   [5] status, [6-18] identity.
fn make_record_path(
    entity_type: u16,
    origin: &str,
    creator: &str,
    status: u16,
    id: &str,
    timestamp_ns: u64,
) -> tagma_core::CoordPath<STORE_DEPTH> {
    use tagma_core::{Coord, CoordPath};

    fn val(v: u16) -> Coord {
        Coord::new(v % 11172).expect("coord in range")
    }

    let mut coords = [Coord::new(0).unwrap(); STORE_DEPTH];

    let days = (timestamp_ns / 86_400_000_000_000) as u16;
    let secs = (timestamp_ns % 86_400_000_000_000 / 1_000_000_000) as u16;
    coords[0] = val(days);
    coords[1] = val(secs);
    coords[2] = val(entity_type);
    coords[3] = hash_str_to_coord(origin);
    coords[4] = hash_str_to_coord(creator);
    coords[5] = val(status);

    // [6-11]: identity from CoordId<6> coordinates directly.
    let cid: CoordId = CoordId::from_string(id);
    let cid_coords: &[tagma_core::Coord; 6] = cid.0.coords();
    for i in 0..6 {
        coords[6 + i] = cid_coords[i];
    }
    // [12-18]: zero padding.
    let zero = Coord::new(0).unwrap();
    for i in 12..STORE_DEPTH {
        coords[i] = zero;
    }

    CoordPath::new(coords)
}

fn hash_str_to_coord(s: &str) -> tagma_core::Coord {
    use tagma_core::Coord;
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let hash = h.finalize();
    Coord::new(u16::from_le_bytes([hash[0], hash[1]]) % 11172)
        .expect("hash coord in range")
}

// ── Blob IO ───────────────────────────────────────────────────────

async fn write_blob_meta(io: &SimIo, blob_hash: &str, mime: &str, size: usize) {
    let meta = ContentMeta {
        mime_type: mime.into(),
        size: size as u64,
    };
    let meta_bytes = postcard::to_allocvec(&meta).unwrap_or_default();
    let _ = io
        .write(&format!("blob/{}.bin.meta", blob_hash), &meta_bytes)
        .await;
}

fn content_hash(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

fn nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

// ── ID generation ─────────────────────────────────────────────────

fn make_number_fact_id(value: i64) -> CoordId {
    CoordId::from_string(&format!(
        "calc/num/{}",
        content_hash(&value.to_le_bytes())
    ))
}

fn make_intent_id(op: OpType, lhs: &CoordId, rhs: &CoordId) -> CoordId {
    CoordId::from_string(&format!("calc/op/{}/{}/{}", op, lhs, rhs))
}

fn make_hint_id(constraint: &Constraint) -> CoordId {
    CoordId::from_string(&format!("calc/hint/{}", constraint))
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_put_and_get() {
        let engine = CalcEngine::new();
        let id = engine.put(42).await;
        assert_eq!(engine.get(&id).await, Some(42));
    }

    #[tokio::test]
    async fn test_put_deduplicates() {
        let engine = CalcEngine::new();
        let id1 = engine.put(42).await;
        let id2 = engine.put(42).await;
        assert_eq!(id1, id2);
        assert_eq!(engine.fact_count().await, 1);
    }

    #[tokio::test]
    async fn test_add_intent_and_resolve() {
        let engine = CalcEngine::new();
        let a = engine.put(3).await;
        let b = engine.put(5).await;
        let intent_id = engine.op(OpType::Add, &a, &b).await.unwrap();
        let resolved = engine.resolve(&intent_id).await.unwrap();
        assert_eq!(resolved.result_value, 8);
        assert_eq!(resolved.op, OpType::Add);
    }

    #[tokio::test]
    async fn test_subtract() {
        let engine = CalcEngine::new();
        let a = engine.put(10).await;
        let b = engine.put(3).await;
        let intent_id = engine.op(OpType::Sub, &a, &b).await.unwrap();
        let resolved = engine.resolve(&intent_id).await.unwrap();
        assert_eq!(resolved.result_value, 7);
    }

    #[tokio::test]
    async fn test_multiply() {
        let engine = CalcEngine::new();
        let a = engine.put(6).await;
        let b = engine.put(7).await;
        let intent_id = engine.op(OpType::Mul, &a, &b).await.unwrap();
        let resolved = engine.resolve(&intent_id).await.unwrap();
        assert_eq!(resolved.result_value, 42);
    }

    #[tokio::test]
    async fn test_divide() {
        let engine = CalcEngine::new();
        let a = engine.put(42).await;
        let b = engine.put(6).await;
        let intent_id = engine.op(OpType::Div, &a, &b).await.unwrap();
        let resolved = engine.resolve(&intent_id).await.unwrap();
        assert_eq!(resolved.result_value, 7);
    }

    #[tokio::test]
    async fn test_division_by_zero() {
        let engine = CalcEngine::new();
        let a = engine.put(10).await;
        let b = engine.put(0).await;
        let intent_id = engine.op(OpType::Div, &a, &b).await.unwrap();
        let result = engine.resolve(&intent_id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_constraint_violation() {
        let engine = CalcEngine::new();
        engine.constrain(Constraint::GreaterThan(10)).await;
        let a = engine.put(3).await;
        let b = engine.put(5).await;
        let intent_id = engine.op(OpType::Add, &a, &b).await.unwrap();
        let result = engine.resolve(&intent_id).await;
        assert!(matches!(result, Err(CalcError::ConstraintViolated { .. })));
    }

    #[tokio::test]
    async fn test_constraint_satisfied() {
        let engine = CalcEngine::new();
        engine.constrain(Constraint::GreaterThan(5)).await;
        let a = engine.put(3).await;
        let b = engine.put(5).await;
        let intent_id = engine.op(OpType::Add, &a, &b).await.unwrap();
        let resolved = engine.resolve(&intent_id).await.unwrap();
        assert_eq!(resolved.result_value, 8);
    }

    #[tokio::test]
    async fn test_map_double_transform() {
        let engine = CalcEngine::new();
        engine.constrain(Constraint::MapDouble).await;
        let a = engine.put(3).await;
        let b = engine.put(5).await;
        let intent_id = engine.op(OpType::Add, &a, &b).await.unwrap();
        let resolved = engine.resolve(&intent_id).await.unwrap();
        assert_eq!(resolved.result_value, 16); // (3*2)+(5*2)
    }

    #[tokio::test]
    async fn test_already_resolved() {
        let engine = CalcEngine::new();
        let a = engine.put(1).await;
        let b = engine.put(2).await;
        let intent_id = engine.op(OpType::Add, &a, &b).await.unwrap();
        engine.resolve(&intent_id).await.unwrap();
        let result = engine.resolve(&intent_id).await;
        assert!(matches!(result, Err(CalcError::AlreadyResolved(_))));
    }

    #[tokio::test]
    async fn test_result_fact_persists() {
        let engine = CalcEngine::new();
        let a = engine.put(7).await;
        let b = engine.put(8).await;
        let intent_id = engine.op(OpType::Add, &a, &b).await.unwrap();
        let resolved = engine.resolve(&intent_id).await.unwrap();
        assert_eq!(engine.get(&resolved.result_id).await, Some(15));
        assert_eq!(engine.get(&a).await, Some(7));
        assert_eq!(engine.get(&b).await, Some(8));
    }

    #[tokio::test]
    async fn test_clear_hints() {
        let engine = CalcEngine::new();
        engine.constrain(Constraint::GreaterThan(10)).await;
        engine.clear_hints().await;
        let a = engine.put(3).await;
        let b = engine.put(5).await;
        let intent_id = engine.op(OpType::Add, &a, &b).await.unwrap();
        let resolved = engine.resolve(&intent_id).await.unwrap();
        assert_eq!(resolved.result_value, 8);
    }

    #[tokio::test]
    async fn test_multiple_hints() {
        let engine = CalcEngine::new();
        engine.constrain(Constraint::MapDouble).await;
        engine.constrain(Constraint::GreaterThan(10)).await;
        let a = engine.put(2).await;
        let b = engine.put(3).await;
        let intent_id = engine.op(OpType::Add, &a, &b).await.unwrap();
        // (2*2)+(3*2)=10, 10 > 10 is false -> should fail
        let result = engine.resolve(&intent_id).await;
        assert!(result.is_err());
    }
}
