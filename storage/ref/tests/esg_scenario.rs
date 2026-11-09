// The ESG scenario the demonstration reads, authored here so the volume can be rebuilt.
//
// The volume is committed data, so this is not part of the gate: it writes the bytes a person
// later commits. Run it deliberately:
//
//   cargo test -p nexus-storage-ref --test esg_scenario -- --ignored --nocapture
//
// It writes `storage/ref/demo/esg-volume`, replacing what is there.
//
// The scenario is one reporting period of a synthetic manufacturer: three measured facts, two
// transformations claimed and concluded by the reporting controller, and the requirement the
// results answer. Every number below is a fixture, and the arithmetic in the transformation
// descriptions is the only claim the data makes.

use futures_executor::block_on;
use nex_fih::FihId20 as _;
use nex_fih::{
    AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, Content, CoordId, Fact, Hint, Intent,
};
use nex_io::fs_io::FsIo;
use nexus_storage_ref::RefFihStorage;
use serde_json::json;
use std::path::PathBuf;

fn volume() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("demo/esg-volume")
}

fn content(body: serde_json::Value) -> Content {
    Content {
        mime_type: "application/json".into(),
        data: serde_json::to_vec(&body).expect("the fixture serializes"),
    }
}

fn intent(label: &str, from: Vec<CoordId>, description: &str) -> Intent {
    Intent {
        id: CoordId::from_label(label),
        from_facts: from,
        to_fact_id: None,
        description: description.into(),
        creator: "sustainability-controller".into(),
        worker: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    }
}

#[test]
#[ignore = "authors the committed ESG volume rather than checking anything"]
fn author_the_esg_scenario() {
    let dir = volume();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the volume directory is created");
    let storage =
        RefFihStorage::with_auto_flush(FsIo::new(&dir).expect("the medium opens"), "esg-demo");

    // The measured source records.
    //
    // One plant, one quarter. The meters and the service record are the origins, so a reader
    // can ask who reported each number and when.

    let electricity = block_on(storage.submit_fact(&Fact::new(
        "did:example:plant-li-1".into(),
        content(json!({
            "asset_id": "plant-li-1",
            "event": "electricity",
            "period": "2026-Q1",
            "value_kwh": 412_000,
            "source": "meter:m17",
            "observed_at": "2026-03-31T23:59:00Z"
        })),
        "meter:m17".into(),
    )))
    .expect("the electricity reading is accepted");

    let gas = block_on(storage.submit_fact(&Fact::new(
        "did:example:plant-li-1".into(),
        content(json!({
            "asset_id": "plant-li-1",
            "event": "natural_gas",
            "period": "2026-Q1",
            "value_kwh": 96_400,
            "source": "meter:g04",
            "observed_at": "2026-03-31T23:59:00Z"
        })),
        "meter:g04".into(),
    )))
    .expect("the gas reading is accepted");

    let refrigerant = block_on(storage.submit_fact(&Fact::new(
        "did:example:plant-li-1".into(),
        content(json!({
            "asset_id": "plant-li-1",
            "event": "refrigerant_topup",
            "substance": "R-410A",
            "value_kg": 12.5,
            "source": "service:s-2026-014",
            "observed_at": "2026-02-14T09:20:00Z"
        })),
        "service:contractor".into(),
    )))
    .expect("the refrigerant record is accepted");

    // The transformations, claimed and concluded.
    //
    // A conclusion is a fact about the intent, so the result is filed under the intent's own
    // address and the chain from the sources to the result is the intent's `from_facts`.

    let scope2 = intent(
        "intent_scope2_2026q1",
        vec![electricity],
        "apply the 2026 residual-mix factor (0.2053 kgCO2e/kWh) to the metered electricity and \
         aggregate Scope 2 for 2026-Q1",
    );
    let scope2_id = block_on(storage.submit_intent(&scope2)).expect("the intent is accepted");
    let scope2_text = scope2_id.to_string();
    block_on(storage.claim_intent(&scope2_text, "sustainability-controller"))
        .expect("the controller claims the intent");
    block_on(
        storage.conclude_intent(
            &scope2_text,
            &json!({
                "scope": 2,
                "period": "2026-Q1",
                "tco2e": 84.58,
                "factor_kg_per_kwh": 0.2053,
                "basis": "market-based",
                "source_fact": electricity.to_string()
            })
            .to_string(),
        ),
    )
    .expect("the conclusion is recorded");

    let scope1 = intent(
        "intent_scope1_2026q1",
        vec![gas, refrigerant],
        "convert combustion (0.2016 kgCO2e/kWh) and the R-410A top-up (GWP 2088) to Scope 1 for \
         2026-Q1",
    );
    let scope1_id = block_on(storage.submit_intent(&scope1)).expect("the intent is accepted");
    let scope1_text = scope1_id.to_string();
    block_on(storage.claim_intent(&scope1_text, "sustainability-controller"))
        .expect("the controller claims the intent");
    block_on(
        storage.conclude_intent(
            &scope1_text,
            &json!({
                "scope": 1,
                "period": "2026-Q1",
                "tco2e": 45.53,
                "components": { "gas_tco2e": 19.43, "refrigerant_tco2e": 26.10 },
                "source_facts": [gas.to_string(), refrigerant.to_string()]
            })
            .to_string(),
        ),
    )
    .expect("the conclusion is recorded");

    // The requirement the results answer.

    block_on(
        storage.submit_hint(&Hint {
            id: CoordId::from_label("esrs_e1_6"),
            content: "ESRS E1-6: disclose gross Scope 1 and Scope 2 emissions for the reporting \
                  period, with the conversion factors used and the base year stated"
                .into(),
            creator: "regulatory-baseline".into(),
        }),
    )
    .expect("the requirement is recorded");

    block_on(storage.flush_pending()).expect("every write reaches the medium");
    println!("wrote {}", dir.display());
}
