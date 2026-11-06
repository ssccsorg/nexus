// ── The reference store ────────────────────────────────────────────────
//
// This is the reference implementation of the storage contract, not a storage engine. It
// holds the FIH records in maps, writes them at the keys the format publishes, and answers
// the read surface a state network names. A consumer reads it to learn the shape of an
// implementation: submit a fact, flush it, open the volume again, walk it, read one record
// by its address.
//
// What it deliberately does not have is the part that is an implementation's own work: an
// index over the axes, a walk that reads the medium a page at a time, a bounded device
// read, a content cache with an eviction policy, a write-ahead path, or a recovery format.
// Those are the things a storage implementation sells, and this file is the shape one
// answers with rather than the implementation itself. See
// `docs/2026-11-06-the-open-layer-is-a-fabric-and-not-a-store.md`.
//
// What stays true here is the contract: a fact's address is derived from what it holds, a
// record lives at the key its address implies, a walk visits records in key order, and a
// write is visible to a read of the same instance before it reaches the medium.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ops::Range;

use sha2::{Digest, Sha256};

use nex_core::Now;

use nex_fih::Cell2;
use crate::record::{
    ContentMeta, FactRecord, HintRecord, IntentRecord, IntentStatus, RecordTimeUnit,
    decode_hint_record, decode_intent_record,
};
use nex_fih::io::file_io::{FileIo, WriteOp};
use nex_fih::semantic::record::{Query, RecordLoad};
use nex_fih::{
    BlackboardError, BoardState, Content, CoordId, Fact, FihHash, Hint, Intent, PartitionData,
    StateFilter,
};
use nex_fih::FihId20 as _;

/// The payload a caller places directly, which is a record it already holds in pieces.
///
/// This is the write side of the reference store: a caller that has a fact, an intent, or a
/// hint in hand hands it over, and the maps hold it. The medium is the caller's to write, or
/// `flush_pending` is, depending on how the record was built.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub enum Record {
    Fact {
        content: Content,
        content_hash: FihHash,
        origin: String,
        creator: String,
        /// Submission time in nanoseconds since the UNIX epoch.
        submitted_at: u64,
    },
    Intent {
        from_facts: Vec<String>,
        description_hash: String,
        creator: String,
        status: IntentStatus,
        /// Creation time in nanoseconds since the UNIX epoch.
        created_at: u64,
    },
    Hint {
        content: String,
        creator: String,
        /// Submission time in nanoseconds since the UNIX epoch.
        submitted_at: u64,
    },
}

/// Reference storage over an IO channel.
///
/// The maps are the record layer: one keyed by identifier per record type, rebuilt from the
/// medium on `rebuild_cache` and updated on every write. A write is buffered in `pending`
/// and reaches the medium on `flush_pending`; a read materializes a payload from the buffer
/// first, so a session reads what it has written.
pub struct RefFihStorage<I: FileIo> {
    pub io: I,
    project_id: String,
    clock: Box<dyn Now + Send + Sync>,
    /// Whether a write flushes on its own, which is what a deployment that cannot lose a
    /// record asks for.
    auto_flush: bool,
    pub fact_records: Cell2<BTreeMap<String, FactRecord>>,
    pub intent_records: Cell2<BTreeMap<String, IntentRecord>>,
    pub hint_records: Cell2<BTreeMap<String, HintRecord>>,
    /// Writes waiting for `flush_pending` to commit them.
    pub(crate) pending: Cell2<Vec<WriteOp>>,
    semantic_stores: Cell2<Vec<nex_fih::semantic::DynSemanticStore>>,
    /// The fact id each semantic index was assigned to, in index order.
    semantic_ids: Cell2<Vec<String>>,
}

impl<I: FileIo> RefFihStorage<I> {
    /// Create storage with a system wall clock. Std-only.
    #[cfg(feature = "std")]
    pub fn new(io: I, project_id: &str) -> Self {
        Self::with_clock(io, project_id, Box::new(nex_core::SystemClock))
    }

    /// Create storage over an injected clock, which is what a target without a system
    /// clock supplies.
    pub fn with_clock(io: I, project_id: &str, clock: Box<dyn Now + Send + Sync>) -> Self {
        Self::with_options(io, project_id, clock, false)
    }

    /// Create storage that flushes every write, at the cost of batching.
    #[cfg(feature = "std")]
    pub fn with_auto_flush(io: I, project_id: &str) -> Self {
        Self::with_options(io, project_id, Box::new(nex_core::SystemClock), true)
    }

    /// The state-only constructor, named for the read path it serves.
    pub fn with_clock_and_memory(
        io: I,
        project_id: &str,
        clock: Box<dyn Now + Send + Sync>,
    ) -> Self {
        Self::with_options(io, project_id, clock, false)
    }

    /// The full constructor. Kept because a caller that names every option should not have
    /// to know which ones this implementation ignores.
    pub fn with_all(
        io: I,
        project_id: &str,
        clock: Box<dyn Now + Send + Sync>,
        auto_flush: bool,
    ) -> Self {
        Self::with_options(io, project_id, clock, auto_flush)
    }

    fn with_options(
        io: I,
        project_id: &str,
        clock: Box<dyn Now + Send + Sync>,
        auto_flush: bool,
    ) -> Self {
        Self {
            io,
            project_id: project_id.to_string(),
            clock,
            auto_flush,
            fact_records: Cell2::new(BTreeMap::new()),
            intent_records: Cell2::new(BTreeMap::new()),
            hint_records: Cell2::new(BTreeMap::new()),
            pending: Cell2::new(Vec::new()),
            semantic_stores: Cell2::new(Vec::new()),
            semantic_ids: Cell2::new(Vec::new()),
        }
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// Commit the buffered writes to the medium.
    pub async fn flush_pending(&self) -> Result<(), String> {
        let ops = {
            let mut pending = self.pending.borrow_mut();
            if pending.is_empty() {
                return Ok(());
            }
            core::mem::take(&mut *pending)
        };
        if let Err(e) = self.io.apply_batch(&ops).await {
            let mut pending = self.pending.borrow_mut();
            pending.splice(0..0, ops);
            return Err(e);
        }
        Ok(())
    }

    async fn flush_if_auto(&self) -> Result<(), String> {
        if self.auto_flush {
            self.flush_pending().await?;
        }
        Ok(())
    }

    /// Build the maps from what the medium holds. This is the cold-start read: one listing
    /// and one read a record.
    pub async fn rebuild_cache(&self) -> Result<(), String> {
        let mut facts: BTreeMap<String, FactRecord> = BTreeMap::new();
        for key in self.io.list("facts/").await? {
            if let Some(bytes) = self.io.read(&key).await?
                && let Ok(mut record) = postcard::from_bytes::<FactRecord>(&bytes)
            {
                // A record written by an older path can name a blob hash that is not a
                // digest. The bytes it names are the truth, so the map holds the digest the
                // bytes compute to, which is what the conflict guard compares.
                if Self::hex_blob_hash(&record.blob_hash).is_none()
                    && let Ok(Some(data)) = self
                        .io
                        .read(&format!("blob/{}.bin", record.blob_hash))
                        .await
                {
                    let mut h = Sha256::new();
                    h.update(&data);
                    record.blob_hash = format!("{:x}", h.finalize());
                }
                facts.insert(record.id.clone(), record);
            }
        }
        let mut intents: BTreeMap<String, IntentRecord> = BTreeMap::new();
        for key in self.io.list("intents/").await? {
            if let Some(bytes) = self.io.read(&key).await?
                && let Ok(record) = decode_intent_record(&bytes)
            {
                intents.insert(record.id.clone(), record);
            }
        }
        let mut hints: BTreeMap<String, HintRecord> = BTreeMap::new();
        for key in self.io.list("hints/").await? {
            if let Some(bytes) = self.io.read(&key).await?
                && let Ok(record) = decode_hint_record(&bytes)
            {
                hints.insert(record.id.clone(), record);
            }
        }
        *self.fact_records.borrow_mut() = facts;
        *self.intent_records.borrow_mut() = intents;
        *self.hint_records.borrow_mut() = hints;
        Ok(())
    }

    /// The state, assembled from the record maps in identifier order.
    ///
    /// A read commits what this session buffered first. A medium that refuses the write does
    /// not refuse the read: the refusal is logged, the buffer is dropped, and the record stays
    /// in the maps without its payload reaching the medium.
    pub async fn read_state(&self) -> BoardState {
        if let Err(error) = self.flush_pending().await {
            log::warn!("read_state: {error}");
            self.pending.borrow_mut().clear();
        }
        self.materialize(self.state_from_maps()).await
    }

    /// The same state without its payloads and descriptions, which is what a caller that
    /// only needs the shape asks for.
    pub async fn read_state_struct(&self) -> BoardState {
        self.state_from_maps()
    }

    /// The state as the records state it, with no payload read.
    fn state_from_maps(&self) -> BoardState {
        let facts = self
            .fact_records
            .borrow()
            .values()
            .map(|r| Fact {
                id: CoordId::resolve(&r.id),
                origin: r.origin.clone(),
                content_hash: Self::blob_hash_or_zero(&r.blob_hash),
                content: Content {
                    mime_type: "application/json".into(),
                    data: Vec::new(),
                },
                creator: r.creator.clone(),
            })
            .collect();
        let intents = self
            .intent_records
            .borrow()
            .values()
            .map(|r| {
                let mut intent = intent_model(r);
                if !r.description_hash.is_empty() {
                    intent.description = String::new();
                }
                intent
            })
            .collect();
        let hints = self
            .hint_records
            .borrow()
            .values()
            .map(|r| Hint {
                id: CoordId::resolve(&r.id),
                content: r.content.clone(),
                creator: r.creator.clone(),
            })
            .collect();
        BoardState {
            facts,
            intents,
            hints,
        }
    }

    /// Read the payload each fact and intent names, and settle a hash the record could not
    /// state.
    async fn materialize(&self, mut state: BoardState) -> BoardState {
        for fact in &mut state.facts {
            let Some(record) = self
                .fact_records
                .borrow()
                .get(&fact.id.to_string())
                .cloned()
            else {
                continue;
            };
            fact.content = self
                .load_content(&record.blob_hash)
                .await;
            if Self::hex_blob_hash(&record.blob_hash).is_none()
                && !fact.content.data.is_empty()
            {
                let mut h = Sha256::new();
                h.update(&fact.content.data);
                fact.content_hash = FihHash(h.finalize().into());
            }
        }
        for intent in &mut state.intents {
            let Some(record) = self
                .intent_records
                .borrow()
                .get(&intent.id.to_string())
                .cloned()
            else {
                continue;
            };
            if record.description_hash.is_empty() {
                continue;
            }
            let content = self
                .load_content(&record.description_hash)
                .await;
            intent.description = String::from_utf8_lossy(&content.data).to_string();
        }
        state
    }

    /// Place a record a caller built.
    ///
    /// A caller that writes its own blob and builds its own record uses this instead of
    /// `submit_fact`, which derives the record from a model. The maps hold what is placed;
    /// nothing reaches the medium here.
    pub fn place_record(&self, id: &str, record: Record) {
        match record {
            Record::Fact {
                content_hash,
                origin,
                creator,
                submitted_at,
                ..
            } => {
                self.fact_records.borrow_mut().insert(
                    id.to_string(),
                    FactRecord {
                        id: id.to_string(),
                        blob_hash: content_hash.to_string(),
                        origin,
                        creator,
                        submitted_at,
                    },
                );
            }
            Record::Intent {
                from_facts,
                description_hash,
                creator,
                status,
                created_at,
            } => {
                self.intent_records.borrow_mut().insert(
                    id.to_string(),
                    IntentRecord {
                        id: id.to_string(),
                        from_facts,
                        description_hash,
                        creator,
                        status,
                        created_at,
                        time_unit: RecordTimeUnit::Nanos,
                    },
                );
            }
            Record::Hint {
                content,
                creator,
                submitted_at,
            } => {
                self.hint_records.borrow_mut().insert(
                    id.to_string(),
                    HintRecord {
                        id: id.to_string(),
                        content,
                        creator,
                        submitted_at,
                        ttl_secs: None,
                        time_unit: RecordTimeUnit::Nanos,
                    },
                );
            }
        }
    }

    /// Drop a record a caller placed.
    pub fn vacate_record(&self, id: &str) {
        self.fact_records.borrow_mut().remove(id);
        self.intent_records.borrow_mut().remove(id);
        self.hint_records.borrow_mut().remove(id);
    }

    /// Read one fact by identifier: its payload and the framing beside it.
    pub async fn get_fact_by_id(&self, id: &str) -> Option<(Content, FihHash, String, String)> {
        let r = self.get_fact_framing(id).await?;
        let content = self.load_content(&r.blob_hash).await;
        Some((
            content,
            Self::blob_hash_or_zero(&r.blob_hash),
            r.origin,
            r.creator,
        ))
    }

    /// Read one fact's framing, without its payload.
    pub async fn get_fact_framing(&self, id: &str) -> Option<FactRecord> {
        let normalized = CoordId::resolve(id).to_string();
        if let Some(r) = self.fact_records.borrow().get(&normalized).cloned() {
            return Some(r);
        }
        let key = FactRecord::fact_key(&normalized);
        self.io
            .read(&key)
            .await
            .ok()
            .flatten()
            .and_then(|bytes| postcard::from_bytes::<FactRecord>(&bytes).ok())
    }

    /// Read one intent by identifier, with the description beside its record.
    ///
    /// The map answers for a volume this session holds; a fresh instance reads the medium at
    /// the key the identifier implies, which is the same read a consumer makes after a reopen.
    pub async fn get_intent(&self, id: &str) -> Option<(IntentRecord, Option<String>)> {
        let normalized = CoordId::resolve(id).to_string();
        let record = match self.intent_records.borrow().get(&normalized).cloned() {
            Some(record) => record,
            None => {
                let key = IntentRecord::intent_key(&normalized);
                let bytes = self.io.read(&key).await.ok().flatten()?;
                decode_intent_record(&bytes).ok()?
            }
        };
        let description = if record.description_hash.is_empty() {
            None
        } else {
            let content = self.load_content(&record.description_hash).await;
            Some(String::from_utf8_lossy(&content.data).into_owned())
        };
        Some((record, description))
    }

    /// Read one hint by identifier.
    ///
    /// A hint's content is in its record, so this is one read.
    pub async fn get_hint(&self, id: &str) -> Option<HintRecord> {
        let normalized = CoordId::resolve(id).to_string();
        if let Some(record) = self.hint_records.borrow().get(&normalized).cloned() {
            return Some(record);
        }
        let key = HintRecord::hint_key(&normalized);
        let bytes = self.io.read(&key).await.ok().flatten()?;
        decode_hint_record(&bytes).ok()
    }

    /// The intent's fields as the record map holds them. `created_at` and a status
    /// timestamp are nanoseconds, which is the storage unit.
    pub fn get_intent_by_id(
        &self,
        id: &str,
    ) -> Option<(Vec<String>, String, String, IntentStatus, u64)> {
        let normalized = CoordId::resolve(id).to_string();
        let r = self.intent_records.borrow().get(&normalized).cloned()?;
        Some((
            r.from_facts,
            r.description_hash,
            r.creator,
            r.status,
            r.created_at,
        ))
    }

    /// The hint's content, creator, and submission time.
    pub fn get_hint_by_id(&self, id: &str) -> Option<(String, String, u64)> {
        let normalized = CoordId::resolve(id).to_string();
        self.hint_records
            .borrow()
            .get(&normalized)
            .map(|r| (r.content.clone(), r.creator.clone(), r.submitted_at))
    }

    pub fn fact_exists(&self, id: &str) -> bool {
        self.fact_records
            .borrow()
            .contains_key(&CoordId::resolve(id).to_string())
    }

    pub fn intent_exists(&self, id: &str) -> bool {
        self.intent_records
            .borrow()
            .contains_key(&CoordId::resolve(id).to_string())
    }

    pub fn hint_exists(&self, id: &str) -> bool {
        self.hint_records
            .borrow()
            .contains_key(&CoordId::resolve(id).to_string())
    }

    pub fn all_fact_ids(&self) -> Vec<String> {
        self.fact_records.borrow().keys().cloned().collect()
    }

    pub fn all_intent_ids(&self) -> Vec<String> {
        self.intent_records.borrow().keys().cloned().collect()
    }

    pub fn all_hint_ids(&self) -> Vec<String> {
        self.hint_records.borrow().keys().cloned().collect()
    }

    /// The intents that name `fact_id`, which is one scan of the intent map.
    pub fn intents_by_fact(&self, fact_id: &str) -> Vec<String> {
        let normalized = CoordId::resolve(fact_id).to_string();
        self.intent_records
            .borrow()
            .values()
            .filter(|r| r.from_facts.iter().any(|f| f == &normalized))
            .map(|r| r.id.clone())
            .collect()
    }

    pub async fn for_each_fact<F>(&self, visit: F) -> Result<(), String>
    where
        F: FnMut(&FactRecord, Content) -> bool,
    {
        let mut visit = visit;
        for record in self.walk_records().await? {
            let content = self
                .load_content(&record.blob_hash)
                .await;
            if !visit(&record, content) {
                break;
            }
        }
        Ok(())
    }

    pub async fn for_each_fact_record<F>(&self, visit: F) -> Result<(), String>
    where
        F: FnMut(&FactRecord) -> bool,
    {
        let mut visit = visit;
        for record in self.walk_records().await? {
            if !visit(&record) {
                break;
            }
        }
        Ok(())
    }

    /// The fact records the volume holds, in key order, read one at a time.
    ///
    /// The walk reads the medium rather than the map, so a session that has read nothing still
    /// answers for a volume another session wrote. A write this session buffered but could not
    /// commit is overlaid on what the medium holds: a part that refuses a write must not turn a
    /// record the session is still holding into one that is gone.
    async fn walk_records(&self) -> Result<Vec<FactRecord>, String> {
        let mut records: BTreeMap<String, FactRecord> = BTreeMap::new();
        for key in self.io.list("facts/").await? {
            if let Some(bytes) = self.io.read(&key).await?
                && let Ok(record) = postcard::from_bytes::<FactRecord>(&bytes)
            {
                records.insert(key, record);
            }
        }
        for op in self.pending.borrow().iter() {
            match op {
                WriteOp::Write { path, data } if path.starts_with("facts/") => {
                    if let Ok(record) = postcard::from_bytes::<FactRecord>(data) {
                        records.insert(path.clone(), record);
                    }
                }
                WriteOp::Delete { path } if path.starts_with("facts/") => {
                    records.remove(path);
                }
                _ => {}
            }
        }
        Ok(records.into_values().collect())
    }

    /// The filtered read, applied to the maps.
    pub async fn read_state_filtered(&self, filter: &StateFilter) -> BoardState {
        let since = filter.since.as_ref().and_then(|s| s.parse::<u64>().ok());
        let until = filter.until.as_ref().and_then(|s| s.parse::<u64>().ok());

        let mut facts = Vec::new();
        for r in self.fact_records.borrow().values() {
            if let Some(want) = filter.origin.as_deref()
                && r.origin != want
            {
                continue;
            }
            if let Some(want) = filter.creator.as_deref()
                && r.creator != want
            {
                continue;
            }
            if let Some(ts) = since
                && r.submitted_at < ts
            {
                continue;
            }
            if let Some(ts) = until
                && r.submitted_at > ts
            {
                continue;
            }
            if let Some(ids) = filter.fact_ids.as_ref()
                && !ids.iter().any(|id| CoordId::resolve(id).to_string() == r.id)
            {
                continue;
            }
            let content = self.load_content(&r.blob_hash).await;
            facts.push(Fact {
                id: CoordId::resolve(&r.id),
                origin: r.origin.clone(),
                content_hash: Self::blob_hash_or_zero(&r.blob_hash),
                content,
                creator: r.creator.clone(),
            });
        }

        let mut intents = Vec::new();
        for r in self.intent_records.borrow().values() {
            if let Some(ids) = filter.intent_ids.as_ref()
                && !ids.iter().any(|id| CoordId::resolve(id).to_string() == r.id)
            {
                continue;
            }
            if let Some(want) = filter.creator.as_deref()
                && r.creator != want
            {
                continue;
            }
            if let Some(ts) = since
                && r.created_at < ts
            {
                continue;
            }
            if let Some(ts) = until
                && r.created_at > ts
            {
                continue;
            }
            let status = match &r.status {
                IntentStatus::Submitted => "submitted",
                IntentStatus::Claimed { .. } => "claimed",
                IntentStatus::Concluded { .. } => "concluded",
            };
            if let Some(want) = filter.status.as_deref()
                && status != want
            {
                continue;
            }
            let description = if r.description_hash.is_empty() {
                r.id.clone()
            } else {
                let content = self.load_content(&r.description_hash).await;
                String::from_utf8_lossy(&content.data).to_string()
            };
            intents.push(Intent {
                id: CoordId::resolve(&r.id),
                from_facts: r.from_facts.iter().map(|s| CoordId::resolve(s)).collect(),
                description,
                creator: r.creator.clone(),
                worker: match &r.status {
                    IntentStatus::Claimed { worker, .. } | IntentStatus::Concluded { worker, .. } => {
                        Some(worker.clone())
                    }
                    IntentStatus::Submitted => None,
                },
                to_fact_id: match &r.status {
                    IntentStatus::Concluded { to_fact, .. } => Some(CoordId::resolve(to_fact)),
                    _ => None,
                },
                last_heartbeat_at: match &r.status {
                    IntentStatus::Claimed {
                        last_heartbeat_at, ..
                    } => Some(nanos_to_secs(*last_heartbeat_at)),
                    _ => None,
                },
                created_at: Some(nanos_to_secs(r.created_at)),
                is_concluded: matches!(r.status, IntentStatus::Concluded { .. }),
                concluded_at: match &r.status {
                    IntentStatus::Concluded { concluded_at, .. } => {
                        Some(nanos_to_secs(*concluded_at))
                    }
                    _ => None,
                },
            });
        }

        let mut hints = Vec::new();
        for r in self.hint_records.borrow().values() {
            if let Some(ids) = filter.hint_ids.as_ref()
                && !ids.iter().any(|id| CoordId::resolve(id).to_string() == r.id)
            {
                continue;
            }
            if let Some(want) = filter.creator.as_deref()
                && r.creator != want
            {
                continue;
            }
            hints.push(Hint {
                id: CoordId::resolve(&r.id),
                content: r.content.clone(),
                creator: r.creator.clone(),
            });
        }

        // The window is over a set already narrowed by the predicates, so paging it here
        // costs the answer and not the volume.
        let offset = filter.offset.unwrap_or(0);
        if offset > 0 || filter.limit.is_some() {
            let take = |len: usize| {
                let start = offset.min(len);
                let end = filter.limit.map(|l| (start + l).min(len)).unwrap_or(len);
                (start, end)
            };
            let (f0, f1) = take(facts.len());
            let (i0, i1) = take(intents.len());
            let (h0, h1) = take(hints.len());
            facts = facts[f0..f1].to_vec();
            intents = intents[i0..i1].to_vec();
            hints = hints[h0..h1].to_vec();
        }

        BoardState {
            facts,
            intents,
            hints,
        }
    }

    /// How many records the maps hold.
    pub fn record_count(&self) -> usize {
        self.fact_records.borrow().len()
            + self.intent_records.borrow().len()
            + self.hint_records.borrow().len()
    }

    /// Drop the hints submitted before `before` (a nanosecond timestamp as text).
    pub async fn evict_before(&self, before: &str) -> Result<u64, String> {
        let before_ns: u64 = before.parse().unwrap_or(0);
        let expired: Vec<HintRecord> = self
            .hint_records
            .borrow()
            .values()
            .filter(|r| r.submitted_at < before_ns)
            .cloned()
            .collect();
        let mut evicted = 0u64;
        for record in expired {
            self.pending
                .borrow_mut()
                .push(WriteOp::Delete { path: record.key() });
            self.hint_records.borrow_mut().remove(&record.id);
            evicted += 1;
        }
        self.flush_if_auto().await?;
        Ok(evicted)
    }

    /// Drop the unclaimed intents older than `older_than_ns` nanoseconds.
    pub async fn evict_stale_intents(&self, older_than_ns: u64) -> Result<u64, String> {
        let cutoff = self.clock.now_nanos().saturating_sub(older_than_ns);
        let stale: Vec<IntentRecord> = self
            .intent_records
            .borrow()
            .values()
            .filter(|r| matches!(r.status, IntentStatus::Submitted) && r.created_at < cutoff)
            .cloned()
            .collect();
        let mut evicted = 0u64;
        for record in stale {
            self.pending
                .borrow_mut()
                .push(WriteOp::Delete { path: record.key() });
            self.intent_records.borrow_mut().remove(&record.id);
            evicted += 1;
        }
        self.flush_if_auto().await?;
        Ok(evicted)
    }

    /// The records a partition names, which is the `partition:<name>` prefix on a fact's
    /// origin or on an intent's or hint's creator.
    pub async fn scan_partition(&self, partition: &str) -> Result<PartitionData, String> {
        let named = format!("partition:{partition}");
        let state = self.materialize(self.state_from_maps()).await;
        Ok(PartitionData {
            partition: partition.to_string(),
            facts: state
                .facts
                .into_iter()
                .filter(|f| f.origin == named)
                .collect(),
            intents: state
                .intents
                .into_iter()
                .filter(|i| i.creator == named)
                .collect(),
            hints: state
                .hints
                .into_iter()
                .filter(|h| h.creator == named)
                .collect(),
        })
    }

    /// The span the facts cover, as nanosecond timestamps.
    fn fact_time_range(&self) -> Option<Range<String>> {
        let recs = self.fact_records.borrow();
        let mut iter = recs.values().map(|r| r.submitted_at);
        let first = iter.next()?;
        let (min, max) = iter.fold((first, first), |(lo, hi), t| (lo.min(t), hi.max(t)));
        Some(min.to_string()..max.to_string())
    }

    /// Register a meaning index, which is a derived structure the reference store does not
    /// build on its own.
    pub fn register_semantic_store(&self, store: nex_fih::semantic::DynSemanticStore) {
        self.semantic_stores.borrow_mut().push(store);
    }

    pub fn semantic_stores(
        &self,
    ) -> impl core::ops::Deref<Target = Vec<nex_fih::semantic::DynSemanticStore>> {
        self.semantic_stores.borrow()
    }

    /// The identifier a meaning index assigned to a fact, or the empty string when the
    /// index does not carry that position.
    pub fn resolve_semantic_idx(&self, idx: u32) -> String {
        self.semantic_ids
            .borrow()
            .get(idx as usize)
            .cloned()
            .unwrap_or_default()
    }

    /// Search the registered meaning indexes and keep the best `top_k`.
    pub async fn semantic_search(
        &self,
        query: &dyn Query,
        top_k: usize,
    ) -> Result<Vec<(u32, f32)>, String> {
        let mut stores = core::mem::take(&mut *self.semantic_stores.borrow_mut());
        if stores.is_empty() {
            self.semantic_stores.borrow_mut().extend(stores);
            return Err("no semantic stores configured".into());
        }
        let mut results = Vec::new();
        for store in stores.iter_mut() {
            if let Ok(mut r) = store.search(query, top_k).await {
                results.append(&mut r);
            }
        }
        self.semantic_stores.borrow_mut().extend(stores);
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(core::cmp::Ordering::Equal));
        results.truncate(top_k);
        Ok(results)
    }

    /// Insert one fact's text into every registered meaning index.
    pub async fn semantic_insert(&self, id: u32, load: &dyn RecordLoad) -> Result<(), String> {
        let mut stores = core::mem::take(&mut *self.semantic_stores.borrow_mut());
        if stores.is_empty() {
            self.semantic_stores.borrow_mut().extend(stores);
            return Err("no semantic stores configured".into());
        }
        let mut failure = None;
        for store in stores.iter_mut() {
            if let Err(error) = store.insert(id, load).await {
                failure = Some(error);
                break;
            }
        }
        // Put the indexes back whichever way the insert went: one that refused a record is
        // still registered for the next one.
        self.semantic_stores.borrow_mut().extend(stores);
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Build every registered meaning index from the facts the maps hold.
    pub async fn rebuild_semantic(&self) -> Result<(), String> {
        let mut stores = core::mem::take(&mut *self.semantic_stores.borrow_mut());
        if stores.is_empty() {
            return Ok(());
        }
        let records: Vec<FactRecord> = self.fact_records.borrow().values().cloned().collect();
        self.semantic_ids.borrow_mut().clear();
        for r in records {
            let content = self.load_content(&r.blob_hash).await;
            if content.data.is_empty() {
                continue;
            }
            let text = String::from_utf8_lossy(&content.data).to_string();
            if text.trim().is_empty() {
                continue;
            }
            let idx = {
                let mut ids = self.semantic_ids.borrow_mut();
                let idx = ids.len() as u32;
                ids.push(r.id.clone());
                idx
            };
            let load = TextRecord { text };
            for store in stores.iter_mut() {
                let _ = store.insert(idx, &load).await;
            }
        }
        self.semantic_stores.borrow_mut().extend(stores);
        Ok(())
    }

    /// Read a payload: a buffered write first, then the medium.
    async fn load_content(&self, blob_hash: &str) -> Content {
        if blob_hash.is_empty() {
            return Content {
                mime_type: "application/json".into(),
                data: Vec::new(),
            };
        }
        let data_key = format!("blob/{blob_hash}.bin");
        let meta_key = format!("blob/{blob_hash}.bin.meta");

        // The buffer is a cell guard, which is not Send, so what it holds is copied out and
        // the guard is released before the first await.
        let buffered = {
            let pending = self.pending.borrow();
            let data = pending.iter().rev().find_map(|op| match op {
                WriteOp::Write { path, data } if path == &data_key => Some(data.clone()),
                _ => None,
            });
            data.map(|data| {
                let mime = pending
                    .iter()
                    .rev()
                    .find_map(|op| match op {
                        WriteOp::Write { path, data } if path == &meta_key => {
                            postcard::from_bytes::<ContentMeta>(data).ok().map(|m| m.mime_type)
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| "application/json".to_string());
                (data, mime)
            })
        };
        if let Some((data, mime)) = buffered {
            return Content {
                mime_type: mime,
                data,
            };
        }
        let mime = self
            .io
            .read(&meta_key)
            .await
            .ok()
            .flatten()
            .and_then(|bytes| postcard::from_bytes::<ContentMeta>(&bytes).ok())
            .map(|m| m.mime_type)
            .unwrap_or_else(|| "application/json".into());
        let data = self.io.read(&data_key).await.ok().flatten().unwrap_or_default();
        Content {
            mime_type: mime,
            data,
        }
    }

    fn hex_blob_hash(hex: &str) -> Option<FihHash> {
        let hex = hex.as_bytes();
        if hex.len() != 64 {
            return None;
        }
        let mut bytes = [0u8; 32];
        for i in 0..32 {
            let hi = (hex[i * 2] as char).to_digit(16)?;
            let lo = (hex[i * 2 + 1] as char).to_digit(16)?;
            bytes[i] = ((hi << 4) | lo) as u8;
        }
        Some(FihHash(bytes))
    }

    fn blob_hash_or_zero(blob_hash: &str) -> FihHash {
        match Self::hex_blob_hash(blob_hash) {
            Some(h) => h,
            None => FihHash([0u8; 32]),
        }
    }
}

/// The intent model a record states, without reading its description.
fn intent_model(r: &IntentRecord) -> Intent {
    Intent {
        id: CoordId::resolve(&r.id),
        from_facts: r.from_facts.iter().map(|s| CoordId::resolve(s)).collect(),
        description: if r.description_hash.is_empty() {
            r.id.clone()
        } else {
            String::new()
        },
        creator: r.creator.clone(),
        worker: match &r.status {
            IntentStatus::Claimed { worker, .. } | IntentStatus::Concluded { worker, .. } => {
                Some(worker.clone())
            }
            IntentStatus::Submitted => None,
        },
        to_fact_id: match &r.status {
            IntentStatus::Concluded { to_fact, .. } => Some(CoordId::resolve(to_fact)),
            _ => None,
        },
        last_heartbeat_at: match &r.status {
            IntentStatus::Claimed {
                last_heartbeat_at, ..
            } => Some(nanos_to_secs(*last_heartbeat_at)),
            _ => None,
        },
        created_at: Some(nanos_to_secs(r.created_at)),
        is_concluded: matches!(r.status, IntentStatus::Concluded { .. }),
        concluded_at: match &r.status {
            IntentStatus::Concluded { concluded_at, .. } => Some(nanos_to_secs(*concluded_at)),
            _ => None,
        },
    }
}

struct TextRecord {
    text: String,
}

impl RecordLoad for TextRecord {
    fn content(&self, _id: u32) -> Option<Vec<u8>> {
        Some(self.text.as_bytes().to_vec())
    }
    fn features(&self, _id: u32) -> Option<Vec<f32>> {
        None
    }
}

fn nanos_to_secs(nanos: u64) -> u64 {
    nanos / 1_000_000_000
}

// ── The read surface ───────────────────────────────────────────────────

impl<I: FileIo> nex_fih::AsyncStorageRead for RefFihStorage<I> {
    fn project_id(&self) -> &str {
        &self.project_id
    }

    async fn read_state(&self) -> BoardState {
        RefFihStorage::read_state(self).await
    }
}

impl<I: FileIo> nex_fih::AsyncFactCapable for RefFihStorage<I> {
    async fn submit_fact(&self, fact: &Fact) -> Result<CoordId, BlackboardError> {
        let id = fact.id.to_string();

        // A second fact at one address is legitimate only when it is the same record. The
        // address is derived from content, origin, and creator, so a different payload
        // under one address is a conflict rather than a retry.
        if let Some(record) = self.get_fact_framing(&id).await {
            let same_content = Self::hex_blob_hash(&record.blob_hash) == Some(fact.content_hash);
            let same_identity = record.origin == fact.origin && record.creator == fact.creator;
            if !same_content || !same_identity {
                return Err(BlackboardError::Conflict(format!(
                    "fact id {id} already exists with a different content_hash or identity"
                )));
            }
            return Ok(fact.id);
        }

        let blob_hash = fact.content_hash.to_string();
        let record = FactRecord::from_model(fact, blob_hash.clone(), self.clock.now_nanos());

        self.pending.borrow_mut().push(WriteOp::Write {
            path: format!("blob/{blob_hash}.bin"),
            data: fact.content.data.clone(),
        });
        let meta = ContentMeta {
            mime_type: fact.content.mime_type.clone(),
            size: fact.content.data.len() as u64,
        };
        let meta_bytes =
            postcard::to_allocvec(&meta).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.borrow_mut().push(WriteOp::Write {
            path: format!("blob/{blob_hash}.bin.meta"),
            data: meta_bytes,
        });
        let bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.borrow_mut().push(WriteOp::Write {
            path: record.key(),
            data: bytes,
        });
        self.fact_records
            .borrow_mut()
            .insert(record.id.clone(), record);

        // Index the payload where a meaning index is registered, so a search over what this
        // session wrote does not have to wait for a rebuild.
        if !fact.origin.starts_with("conclusion:") && !self.semantic_stores.borrow().is_empty() {
            let idx = {
                let mut ids = self.semantic_ids.borrow_mut();
                let idx = ids.len() as u32;
                ids.push(fact.id.to_string());
                idx
            };
            let load = TextRecord {
                text: String::from_utf8_lossy(&fact.content.data).to_string(),
            };
            self.semantic_insert(idx, &load).await.ok();
        }

        self.flush_if_auto()
            .await
            .map_err(BlackboardError::Internal)?;
        Ok(fact.id)
    }
}

impl<I: FileIo> nex_fih::AsyncHintCapable for RefFihStorage<I> {
    async fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        let record = HintRecord {
            id: hint.id.to_string(),
            content: hint.content.clone(),
            creator: hint.creator.clone(),
            submitted_at: self.clock.now_nanos(),
            ttl_secs: None,
            time_unit: RecordTimeUnit::Nanos,
        };
        let bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.borrow_mut().push(WriteOp::Write {
            path: record.key(),
            data: bytes,
        });
        self.hint_records.borrow_mut().insert(record.id.clone(), record);
        self.flush_if_auto()
            .await
            .map_err(BlackboardError::Internal)?;
        Ok(())
    }
}

impl<I: FileIo> nex_fih::AsyncIntentCapable for RefFihStorage<I> {
    async fn submit_intent(&self, intent: &Intent) -> Result<CoordId, BlackboardError> {
        // An intent is work over facts, so it names at least one and every one has to exist.
        if intent.from_facts.is_empty() {
            return Err(BlackboardError::Forbidden(
                "an intent names at least one fact".into(),
            ));
        }
        for fact in &intent.from_facts {
            let id = fact.to_string();
            if !self.fact_exists(&id) {
                return Err(BlackboardError::NotFound(format!(
                    "intent references a fact that is not here: {id}"
                )));
            }
        }
        let now = self.clock.now_nanos();
        // An intent with no description carries no blob and no hash: the absence is the
        // record's own statement, and the read reports it as one.
        let description_bytes = intent.description.as_bytes().to_vec();
        let description_hash = if description_bytes.is_empty() {
            String::new()
        } else {
            let mut h = Sha256::new();
            h.update(&description_bytes);
            format!("{:x}", h.finalize())
        };
        let record = IntentRecord {
            id: intent.id.to_string(),
            from_facts: intent.from_facts.iter().map(|f| f.to_string()).collect(),
            description_hash: description_hash.clone(),
            creator: intent.creator.clone(),
            status: IntentStatus::Submitted,
            created_at: now,
            time_unit: RecordTimeUnit::Nanos,
        };

        if !description_hash.is_empty() {
            self.pending.borrow_mut().push(WriteOp::Write {
                path: format!("blob/{description_hash}.bin"),
                data: description_bytes.clone(),
            });
            let meta = ContentMeta {
                mime_type: "application/json".into(),
                size: description_bytes.len() as u64,
            };
            let meta_bytes = postcard::to_allocvec(&meta)
                .map_err(|e| BlackboardError::Internal(e.to_string()))?;
            self.pending.borrow_mut().push(WriteOp::Write {
                path: format!("blob/{description_hash}.bin.meta"),
                data: meta_bytes,
            });
        }
        let bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.borrow_mut().push(WriteOp::Write {
            path: record.key(),
            data: bytes,
        });
        self.intent_records
            .borrow_mut()
            .insert(record.id.clone(), record);
        self.flush_if_auto()
            .await
            .map_err(BlackboardError::Internal)?;
        Ok(intent.id)
    }

    async fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        // The lifecycle reads the record the medium holds, so what this session buffered
        // reaches it first. A flush that fails is an internal error, not a missing intent.
        self.flush_pending()
            .await
            .map_err(BlackboardError::Internal)?;
        self.move_intent(
            intent_id,
            |status| status.try_claim(agent, self.clock.now_nanos()),
            |e| {
                if e.starts_with("already claimed") {
                    BlackboardError::Conflict(e)
                } else {
                    BlackboardError::Internal(e)
                }
            },
        )
        .await
    }

    // `try_claim` reports an occupied claim with a sentence that names it; the rest of the
    // refusals are states the caller reaches by calling out of order.

    async fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        // The lifecycle reads the record the medium holds, so what this session buffered
        // reaches it first. A flush that fails is an internal error, not a missing intent.
        self.flush_pending()
            .await
            .map_err(BlackboardError::Internal)?;
        self.move_intent(
            intent_id,
            |status| status.try_heartbeat(agent, self.clock.now_nanos()),
            |e| {
                if e.contains("not") {
                    BlackboardError::Conflict(e)
                } else {
                    BlackboardError::Internal(e)
                }
            },
        )
        .await
    }

    async fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        // The lifecycle reads the record the medium holds, so what this session buffered
        // reaches it first. A flush that fails is an internal error, not a missing intent.
        self.flush_pending()
            .await
            .map_err(BlackboardError::Internal)?;
        let normalized = CoordId::resolve(intent_id).to_string();
        let record = self
            .intent_records
            .borrow()
            .get(&normalized)
            .cloned()
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent {intent_id} not found")))?;
        let status = match &record.status {
            // Nothing holds an unclaimed intent, so there is no lease to release and the
            // caller gets the state it asked for.
            IntentStatus::Submitted => return Ok(()),
            IntentStatus::Claimed { worker, .. } if worker == agent => IntentStatus::Submitted,
            IntentStatus::Claimed { worker, .. } => {
                return Err(BlackboardError::Forbidden(format!(
                    "intent {intent_id} is claimed by {worker}"
                )));
            }
            IntentStatus::Concluded { .. } => {
                return Err(BlackboardError::NotFound(format!(
                    "intent {intent_id} is concluded"
                )));
            }
        };
        self.write_intent(record, status).await
    }

    async fn conclude_intent(
        &self,
        intent_id: &str,
        result: &str,
    ) -> Result<Fact, BlackboardError> {
        // The lifecycle reads the record the medium holds, so what this session buffered
        // reaches it first. A flush that fails is an internal error, not a missing intent.
        self.flush_pending()
            .await
            .map_err(BlackboardError::Internal)?;
        let normalized = CoordId::resolve(intent_id).to_string();
        let record = self
            .intent_records
            .borrow()
            .get(&normalized)
            .cloned()
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent {intent_id} not found")))?;
        let worker = match &record.status {
            IntentStatus::Claimed { worker, .. } => worker.clone(),
            IntentStatus::Submitted => {
                return Err(BlackboardError::BadRequest(format!(
                    "intent {intent_id} is not claimed"
                )));
            }
            IntentStatus::Concluded { .. } => {
                return Err(BlackboardError::Internal(format!(
                    "intent {intent_id} is already concluded"
                )));
            }
        };

        // The conclusion is a fact about the intent, so its address comes from the intent's
        // canonical identifier: two spellings of one intent must not produce two facts.
        let conclusion_id = CoordId::from_label(&format!("f_concl_{}", record.id));
        let content = Content {
            mime_type: "text/plain".into(),
            data: result.as_bytes().to_vec(),
        };
        let fact = Fact {
            id: conclusion_id,
            content_hash: {
                let mut h = Sha256::new();
                h.update(&content.data);
                FihHash(h.finalize().into())
            },
            origin: format!("conclusion:{}", record.id),
            content: content.clone(),
            creator: worker,
        };

        let now = self.clock.now_nanos();
        let status = record
            .status
            .try_conclude(&conclusion_id.to_string(), now)
            .map_err(BlackboardError::Internal)?;

        nex_fih::AsyncFactCapable::submit_fact(self, &fact).await?;
        self.write_intent(record, status).await?;
        Ok(fact)
    }
}

impl<I: FileIo> RefFihStorage<I> {
    /// Apply a lifecycle move and write the record back.
    ///
    /// The transition reports its refusal as a sentence about the state it found, and the
    /// caller decides whether that state is a conflict the caller could have read or an
    /// internal one it could not.
    async fn move_intent<F, M>(
        &self,
        intent_id: &str,
        step: F,
        map: M,
    ) -> Result<(), BlackboardError>
    where
        F: FnOnce(&IntentStatus) -> Result<IntentStatus, String>,
        M: FnOnce(String) -> BlackboardError,
    {
        let normalized = CoordId::resolve(intent_id).to_string();
        let record = self
            .intent_records
            .borrow()
            .get(&normalized)
            .cloned()
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent {intent_id} not found")))?;
        let status = step(&record.status).map_err(map)?;
        self.write_intent(record, status).await
    }

    async fn write_intent(
        &self,
        mut record: IntentRecord,
        status: IntentStatus,
    ) -> Result<(), BlackboardError> {
        record.status = status;
        let bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.borrow_mut().push(WriteOp::Write {
            path: record.key(),
            data: bytes,
        });
        self.intent_records
            .borrow_mut()
            .insert(record.id.clone(), record);
        // A lifecycle move commits before it returns, so the state a reader sees after a
        // reopen agrees with the state this call reported.
        self.flush_pending()
            .await
            .map_err(BlackboardError::Internal)?;
        Ok(())
    }
}

impl<I: FileIo> nex_fih::AsyncFilterCapable for RefFihStorage<I> {
    async fn read_state_filtered(&self, filter: &StateFilter) -> BoardState {
        RefFihStorage::read_state_filtered(self, filter).await
    }
}

impl<I: FileIo> nex_fih::AsyncEvictCapable for RefFihStorage<I> {
    async fn approximate_size(&self) -> usize {
        RefFihStorage::record_count(self)
    }

    async fn evict_before(&self, before: &str) -> Result<u64, String> {
        RefFihStorage::evict_before(self, before).await
    }

    async fn evict_stale_intents(&self, older_than_ns: u64) -> Result<u64, String> {
        RefFihStorage::evict_stale_intents(self, older_than_ns).await
    }
}

impl<I: FileIo> nex_fih::AsyncScanCapable for RefFihStorage<I> {
    async fn scan_partition(&self, partition: &str) -> Result<PartitionData, String> {
        RefFihStorage::scan_partition(self, partition).await
    }
}

impl<I: FileIo> nex_fih::AsyncTimeRangeCapable for RefFihStorage<I> {
    async fn time_range(&self) -> Option<Range<String>> {
        RefFihStorage::fact_time_range(self)
    }
}

impl<I: FileIo> nex_fih::FactById for RefFihStorage<I> {
    async fn fact_by_id(&self, id: &str) -> Option<(Content, FihHash, String, String)> {
        RefFihStorage::get_fact_by_id(self, id).await
    }

    async fn fact_framing(&self, id: &str) -> Option<nex_fih::FactFraming> {
        RefFihStorage::get_fact_framing(self, id)
            .await
            .map(|record| record.framing())
    }
}

impl<I: FileIo> nex_fih::FactWalk for RefFihStorage<I> {
    async fn for_each_fact<F>(&self, mut visit: F) -> Result<(), String>
    where
        F: FnMut(&nex_fih::FactFraming, Content) -> bool,
    {
        for record in self.walk_records().await? {
            let content = self.load_content(&record.blob_hash).await;
            if !visit(&record.framing(), content) {
                break;
            }
        }
        Ok(())
    }

    async fn for_each_fact_record<F>(&self, mut visit: F) -> Result<(), String>
    where
        F: FnMut(&nex_fih::FactFraming) -> bool,
    {
        for record in self.walk_records().await? {
            if !visit(&record.framing()) {
                break;
            }
        }
        Ok(())
    }
}
