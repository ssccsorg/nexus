// gateway/nex-cf — FihStorage<CfFihIo> over R2.

use nexus_model::{BlackboardError, Content, Fact, FactCapable, FihHash, Intent, IntentCapable, StorageRead};
use nexus_storage_sim::cf_io::CfFihIo;
use nexus_storage_sim::FihStorage;
use worker::*;

static STORE: std::sync::OnceLock<FihStorage<CfFihIo>> = std::sync::OnceLock::new();

fn store() -> &'static FihStorage<CfFihIo> {
    STORE.get().expect("FihStorage not initialized")
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    STORE.get_or_init(|| {
        let bucket = env.bucket("FIH_R2").expect("FIH_R2 bucket binding required");
        FihStorage::with_auto_flush(CfFihIo::new(bucket), "cf-nexus")
    });

    let url = req.url()?;
    let path = url.path().to_string();
    let q: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();

    fn qv(q: &[(String, String)], k: &str) -> String {
        q.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap_or_default()
    }

    if path == "/" { return Response::ok("nexus-cf"); }

    if path == "/state" { return Response::from_json(&store().read_state()); }

    if path == "/fact" {
        let bb = store();
        console_log!("pre-submit: fact_store.len = {}", bb.fact_store.len());
        let hash = bb.submit_fact(&Fact {
            id: FihHash(qv(&q, "id")),
            origin: qv(&q, "origin"),
            content: Content { mime_type: "application/json".into(), data: qv(&q, "content").into_bytes() },
            creator: qv(&q, "creator"),
        }).map_err(|e| Error::RustError(e.to_string()))?;
        console_log!("post-submit: fact_store.len = {}", bb.fact_store.len());
        return Response::from_json(&serde_json::json!({"id": hash.0}));
    }

    if path == "/intent" {
        store().submit_intent(&Intent {
            id: FihHash(qv(&q, "id")),
            from_facts: qv(&q, "from").split(',').filter(|s| !s.is_empty()).map(String::from).collect(),
            description: qv(&q, "desc"), creator: qv(&q, "creator"),
            worker: None, to_fact_id: None, last_heartbeat_at: None,
            created_at: None, is_concluded: false, concluded_at: None,
        }).map_err(|e| Error::RustError(e.to_string()))?;
        return Response::from_json(&serde_json::json!({"status":"ok"}));
    }

    Response::error("not found", 404)
}
