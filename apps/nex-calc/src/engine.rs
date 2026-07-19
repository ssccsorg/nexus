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

use nex::storage::core::intent_status::IntentStatus;
use nex::storage::core::record::{ContentMeta, FactRecord, HintRecord, IntentRecord};
use nex::storage::core::store::FihStorage;
use nex::{EntityStore, FileIo};
use nexus_model::{Content, FihHash};
use nexus_storage_sim::SimIo;

use crate::hint::Constraint;
use crate::ops::OpType;

const NUMBER_MIME: &str = "application/x-nex-calc-number";

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
    pub intent_id: FihHash,
    pub op: OpType,
    pub lhs: i64,
    pub rhs: i64,
    pub result_id: FihHash,
    pub result_value: i64,
}

/// Calculator engine backed by FihStorage<SimIo>.
///
/// All state lives in FihStorage's in-memory stores. The IO layer
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
    pub async fn put(&self, value: i64) -> FihHash {
        let id = make_number_fact_id(value);
        let id_str = id.to_string();
        if self.storage.fact_store.contains_key(&id_str).await {
            return id;
        }

        let data = value.to_le_bytes().to_vec();
        let blob_hash = content_hash(&data);
        let blob_path = format!("blob/{}.bin", blob_hash);

        // Write content blob and metadata via the IO layer.
        let _ = self.storage.io.write(&blob_path, &data).await;
        write_blob_meta(&self.storage.io, &blob_hash, NUMBER_MIME, data.len()).await;

        let record = FactRecord::from_model(
            &nexus_model::Fact::new(
                id,
                "nex-calc".into(),
                Content::from(""),
                "user".into(),
            ),
            blob_hash,
            0,
        );
        self.storage.fact_store.insert(id_str, record).await;
        id
    }

    /// Read a number from a Fact.
    pub async fn get(&self, fact_id: &FihHash) -> Option<i64> {
        let record = self.storage.fact_store.get(&fact_id.to_string()).await?;
        decode_blob(&self.storage.io, &record.blob_hash).await
    }

    /// Look up a Fact by short hex prefix.
    pub async fn find_fact(&self, prefix: &str) -> Option<FihHash> {
        let prefix_lower = prefix.to_lowercase();
        for r in self.storage.fact_store.values().await.iter() {
            if r.id.to_lowercase().starts_with(&prefix_lower) {
                return Some(FihHash::from_hex(&r.id));
            }
        }
        None
    }

    // ── Intent operations ─────────────────────────────────────────

    /// Create an operator Intent. Returns its content-addressed FihHash.
    pub async fn op(
        &self,
        op: OpType,
        lhs_id: &FihHash,
        rhs_id: &FihHash,
    ) -> Result<FihHash, CalcError> {
        if !self
            .storage
            .fact_store
            .contains_key(&lhs_id.to_string())
            .await
        {
            return Err(CalcError::FactNotFound(lhs_id.to_string()));
        }
        if !self
            .storage
            .fact_store
            .contains_key(&rhs_id.to_string())
            .await
        {
            return Err(CalcError::FactNotFound(rhs_id.to_string()));
        }

        let id = make_intent_id(op, lhs_id, rhs_id);
        let id_str = id.to_string();
        if self.storage.intent_store.contains_key(&id_str).await {
            return Ok(id);
        }

        let now = nanos();
        let desc = format!("{}", op);

        let record = IntentRecord {
            id: id_str.clone(),
            from_facts: vec![lhs_id.to_string(), rhs_id.to_string()],
            description_hash: desc, // store operator name directly
            creator: "user".into(),
            status: IntentStatus::Submitted,
            created_at: now,
        };
        self.storage.intent_store.insert(id_str, record).await;
        Ok(id)
    }

    /// Resolve an Intent — this IS the computation. Traverses the FIH space:
    ///
    ///   Fact(lhs) ─┐
    ///               ├── Intent(op) ──→ Fact(result)
    ///   Fact(rhs) ─┘        ↑
    ///                   Hint gates
    pub async fn resolve(&self, intent_id: &FihHash) -> Result<ResolvedIntent, CalcError> {
        let id_str = intent_id.to_string();
        let record = self
            .storage
            .intent_store
            .get(&id_str)
            .await
            .ok_or_else(|| CalcError::IntentNotFound(id_str.clone()))?;

        if matches!(record.status, IntentStatus::Concluded { .. }) {
            return Err(CalcError::AlreadyResolved(id_str));
        }

        let op = OpType::parse(&record.description_hash).ok_or_else(|| {
            CalcError::OpError(format!(
                "unknown operator '{}' in intent {}",
                record.description_hash, id_str
            ))
        })?;

        let lhs_fid = record
            .from_facts
            .first()
            .ok_or_else(|| CalcError::IntentNotFound("missing lhs".into()))?;
        let rhs_fid = record
            .from_facts
            .get(1)
            .ok_or_else(|| CalcError::IntentNotFound("missing rhs".into()))?;

        let lhs = self
            .get(&FihHash::from_hex(lhs_fid))
            .await
            .ok_or_else(|| CalcError::FactNotFound(lhs_fid.clone()))?;
        let rhs = self
            .get(&FihHash::from_hex(rhs_fid))
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
        if !self
            .storage
            .fact_store
            .contains_key(&result_id.to_string())
            .await
        {
            let data = raw_result.to_le_bytes().to_vec();
            let bh = content_hash(&data);
            let _ = self
                .storage
                .io
                .write(&format!("blob/{}.bin", bh), &data)
                .await;
            write_blob_meta(&self.storage.io, &bh, NUMBER_MIME, data.len()).await;
            let rec = FactRecord::from_model(
                &nexus_model::Fact::new(
                    result_id,
                    format!("nex-calc:resolve:{}", intent_id),
                    Content::from(""),
                    "nex-calc".into(),
                ),
                bh,
                0,
            );
            self.storage
                .fact_store
                .insert(result_id.to_string(), rec)
                .await;
        }

        // Mark intent concluded.
        let now = nanos();
        let updated = IntentRecord {
            status: IntentStatus::Concluded {
                to_fact: result_id.to_string(),
                concluded_at: now,
                worker: "nex-calc".into(),
            },
            ..record
        };
        self.storage.intent_store.insert(id_str, updated).await;

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
    pub async fn constrain(&self, constraint: Constraint) -> FihHash {
        let id = make_hint_id(&constraint);
        let id_str = id.to_string();
        if !self.storage.hint_store.contains_key(&id_str).await {
            let record = HintRecord {
                id: id_str.clone(),
                content: constraint.to_string(),
                creator: "user".into(),
                submitted_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                ttl_secs: None,
            };
            self.storage.hint_store.insert(id_str, record).await;
        }
        id
    }

    pub async fn clear_hints(&self) {
        self.storage.hint_store.clear().await;
    }

    // ── Queries ───────────────────────────────────────────────────

    pub async fn list_facts(&self) -> Vec<(FihHash, i64)> {
        let mut out = Vec::new();
        for r in self.storage.fact_store.values().await.iter() {
            if let Some(v) = decode_blob(&self.storage.io, &r.blob_hash).await {
                out.push((FihHash::from_hex(&r.id), v));
            }
        }
        out
    }

    pub async fn list_intents(&self) -> Vec<(FihHash, bool)> {
        self.storage
            .intent_store
            .values()
            .await
            .iter()
            .map(|r| {
                (
                    FihHash::from_hex(&r.id),
                    matches!(r.status, IntentStatus::Concluded { .. }),
                )
            })
            .collect()
    }

    pub async fn list_hints(&self) -> Vec<(FihHash, String)> {
        self.storage
            .hint_store
            .values()
            .await
            .iter()
            .map(|r| (FihHash::from_hex(&r.id), r.content.clone()))
            .collect()
    }

    pub async fn fact_count(&self) -> usize {
        self.storage.fact_store.len().await
    }
    pub async fn pending_count(&self) -> usize {
        self.storage
            .intent_store
            .values()
            .await
            .iter()
            .filter(|r| !matches!(r.status, IntentStatus::Concluded { .. }))
            .count()
    }

    // ── Internal ──────────────────────────────────────────────────

    async fn apply_operand_transforms(&self, mut lhs: i64, mut rhs: i64) -> (i64, i64) {
        for r in self.storage.hint_store.values().await.iter() {
            let c = match Constraint::parse_str(&r.content) {
                Some(c) => c,
                None => continue,
            };
            let (l, r2) = c.transform_operands(lhs, rhs);
            lhs = l;
            rhs = r2;
        }
        (lhs, rhs)
    }

    async fn check_constraints(&self, result: i64) -> Result<(), CalcError> {
        for r in self.storage.hint_store.values().await.iter() {
            let c = match Constraint::parse_str(&r.content) {
                Some(c) => c,
                None => continue,
            };
            if !c.check(result) {
                return Err(CalcError::ConstraintViolated {
                    hint_id: r.id.clone(),
                    constraint: c.to_string(),
                    result,
                });
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

async fn decode_blob(io: &SimIo, blob_hash: &str) -> Option<i64> {
    if blob_hash.is_empty() {
        return None;
    }
    let key = format!("blob/{}.bin", blob_hash);
    let bytes = io.read(&key).await.ok()??;
    if bytes.len() != 8 {
        return None;
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&bytes);
    Some(i64::from_le_bytes(arr))
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

fn make_number_fact_id(value: i64) -> FihHash {
    FihHash::new(&[&value.to_string()], "nex-calc-number")
}

fn make_intent_id(op: OpType, lhs_id: &FihHash, rhs_id: &FihHash) -> FihHash {
    FihHash::new(
        &[&lhs_id.to_string(), &rhs_id.to_string(), op.symbol()],
        "nex-calc-intent",
    )
}

fn make_hint_id(constraint: &Constraint) -> FihHash {
    FihHash::new(&[&constraint.to_string()], "nex-calc-hint")
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
        // (2*2)+(3*2)=10, 10 > 10 is false → should fail
        let result = engine.resolve(&intent_id).await;
        assert!(result.is_err());
    }
}
