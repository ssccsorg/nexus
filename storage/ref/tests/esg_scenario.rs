// The ESG scenario the demonstration reads, authored from the data beside it.
//
// The scenario is `demo/esg.scenario.json`: the site, the measured quantities, the requirement texts, and the
// transformations with the figures they conclude. This file is the writer, so a person who wants a different plant changes
// data rather than Rust, and a person who wants a different shape changes this.
//
// The volume is committed data, so this is not part of the gate when it writes. Run it deliberately:
//
//   cargo test -p nexus-storage-ref --test esg_scenario -- --ignored --nocapture
//
// It writes `storage/ref/demo/esg-volume`, replacing what is there. The check that runs in the gate writes to a temporary
// directory and compares the records with the committed ones, so a change to the data file that is not in the volume
// fails there rather than at a review.
//
// The scenario is one reporting period of a synthetic manufacturer, shaped the way a quarterly inventory is: measured
// quantities from the site's instruments, two scope calculations that report Scope 2 on both a location-based and a
// market-based basis, an intensity metric over the two results, and the disclosure requirement they answer. Every number
// in the data file is a fixture, and the arithmetic in a transformation's description is the only claim the data makes.

use futures_executor::block_on;
use nex_fih::FihId20 as _;
use nex_fih::{
    AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, Content, CoordId, Fact, Hint, Intent,
};
use nex_io::fs_io::FsIo;
use nexus_storage_ref::RefFihStorage;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn demo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("demo")
}

fn volume() -> PathBuf {
    demo().join("esg-volume")
}

/// The scenario as data, which is the whole of what a different plant changes.
#[derive(Deserialize)]
struct Scenario {
    volume: String,
    origin: String,
    site: String,
    period: String,
    quality: String,
    /// The creator of every transformation's conclusion.
    controller: String,
    /// The creator of the requirements.
    baseline: String,
    measurements: Vec<Measurement>,
    transformations: Vec<Transformation>,
    requirements: Vec<Requirement>,
}

#[derive(Deserialize)]
struct Measurement {
    /// The name a transformation cites this record by.
    key: String,
    source: String,
    /// `measured` when it is a reading with a unit, `fugitive` when it is a mass and a warming potential.
    #[serde(default)]
    shape: Option<String>,
    value: f64,
    unit: String,
    metric: String,
    reference: String,
    #[serde(default)]
    substance: Option<String>,
    #[serde(default)]
    gwp: Option<u16>,
    #[serde(default)]
    gwp_source: Option<String>,
}

#[derive(Deserialize)]
struct Transformation {
    label: String,
    /// The records it read, by the key of a measurement or the label of an earlier transformation.
    from: Vec<String>,
    description: String,
    #[serde(default)]
    scope1: Option<Scope1>,
    #[serde(default)]
    scope2: Option<Scope2>,
    #[serde(default)]
    intensity: Option<Intensity>,
}

#[derive(Deserialize)]
struct Requirement {
    label: String,
    content: String,
}

/// A payload as the author wrote it.
///
/// The fields keep the order of the struct, and the screen reads a payload in the order it arrives: a record that leads
/// with its figure is a record whose author said the figure leads. Serializing an unordered map instead would leave the
/// order to the format and put `base_year` where the result belongs. That is why the data file carries values and this
/// file carries the shape.
fn content(body: &impl Serialize) -> Content {
    Content {
        mime_type: "application/json".into(),
        data: serde_json::to_vec(body).expect("the fixture serializes"),
    }
}

/// The same payload as the text a conclusion is recorded with.
fn text(body: &impl Serialize) -> String {
    String::from_utf8(content(body).data).expect("the fixture is text")
}

/// A measured quantity, which is what a meter or a production system reports.
#[derive(Serialize)]
struct Measured {
    value: f64,
    unit: String,
    metric: String,
    period: String,
    site: String,
    reference: String,
    quality: String,
    source: String,
}

/// A refrigerant top-up, which is a mass and a global warming potential rather than an energy reading.
#[derive(Serialize)]
struct Fugitive {
    value: f64,
    unit: String,
    metric: String,
    substance: String,
    gwp: u16,
    gwp_source: String,
    period: String,
    site: String,
    reference: String,
    quality: String,
    source: String,
}

/// Scope 2 as ESRS E1-6 asks for it: the same energy on both bases, with the factors named.
#[derive(Clone, Serialize, Deserialize)]
struct Scope2 {
    market_based_tco2e: f64,
    location_based_tco2e: f64,
    scope: u8,
    period: String,
    consolidation: String,
    base_year: u16,
    restated: bool,
    factor_market_kg_per_kwh: f64,
    factor_location_kg_per_kwh: f64,
    factor_source: String,
    /// The requirement this conclusion answers, which is the edge from a hint to the work it bounds.
    requirement: String,
    /// The source it read, which is the edge from a fact to the work that read it. Filled here, not in the data.
    #[serde(skip_deserializing)]
    source_fact: String,
}

/// Scope 1 split by the source it came from, because combustion and fugitive emissions are reported apart.
#[derive(Clone, Serialize, Deserialize)]
struct Scope1 {
    tco2e: f64,
    stationary_combustion_tco2e: f64,
    fugitive_refrigerant_tco2e: f64,
    scope: u8,
    period: String,
    consolidation: String,
    gwp_source: String,
    base_year: u16,
    restated: bool,
    requirement: String,
    #[serde(skip_deserializing)]
    source_facts: Vec<String>,
}

/// The intensity metric, which is what a reader compares across periods and sites.
#[derive(Clone, Serialize, Deserialize)]
struct Intensity {
    kgco2e_per_unit: f64,
    total_tco2e: f64,
    denominator_units: u32,
    basis: String,
    period: String,
    consolidation: String,
    requirement: String,
    #[serde(skip_deserializing)]
    source_facts: Vec<String>,
}

fn intent(label: &str, from: Vec<CoordId>, description: &str, creator: &str) -> Intent {
    Intent {
        id: CoordId::from_label(label),
        from_facts: from,
        to_fact_id: None,
        description: description.into(),
        creator: creator.into(),
        worker: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    }
}

fn load() -> Scenario {
    let path = demo().join("esg.scenario.json");
    let text = std::fs::read_to_string(&path).expect("the scenario is readable");
    serde_json::from_str(&text).expect("the scenario parses")
}

/// Write the scenario's records into a volume, which is the whole of what this file does.
fn author(dir: &Path, scenario: &Scenario) {
    let storage =
        RefFihStorage::with_auto_flush(FsIo::new(dir).expect("the medium opens"), &scenario.volume);

    // The name a transformation cites, and the address it names.
    let mut written: BTreeMap<String, CoordId> = BTreeMap::new();

    // The measured source records, one reading each. The record's creator is the instrument, so a reader can ask who
    // reported each number, on which reference, and when it was read.
    for each in &scenario.measurements {
        let body = match each.shape.as_deref() {
            Some("fugitive") => content(&Fugitive {
                value: each.value,
                unit: each.unit.clone(),
                metric: each.metric.clone(),
                substance: each.substance.clone().expect("a fugitive reading names its substance"),
                gwp: each.gwp.expect("a fugitive reading states its warming potential"),
                gwp_source: each.gwp_source.clone().expect("a fugitive reading names its source"),
                period: scenario.period.clone(),
                site: scenario.site.clone(),
                reference: each.reference.clone(),
                quality: scenario.quality.clone(),
                source: each.source.clone(),
            }),
            _ => content(&Measured {
                value: each.value,
                unit: each.unit.clone(),
                metric: each.metric.clone(),
                period: scenario.period.clone(),
                site: scenario.site.clone(),
                reference: each.reference.clone(),
                quality: scenario.quality.clone(),
                source: each.source.clone(),
            }),
        };
        let id = block_on(storage.submit_fact(&Fact::new(
            scenario.origin.clone(),
            body,
            each.source.clone(),
        )))
        .expect("the reading is accepted");
        written.insert(each.key.clone(), id);
    }

    // The transformations, claimed and concluded.
    //
    // A conclusion is a fact about the intent, so the result is filed at the intent's own conclusion address and the chain
    // from the sources to the result is the intent's `from_facts`.
    for each in &scenario.transformations {
        let from: Vec<CoordId> = each
            .from
            .iter()
            .map(|key| {
                *written
                    .get(key)
                    .unwrap_or_else(|| panic!("{} cites {key}, which nothing writes", each.label))
            })
            .collect();
        let sources: Vec<String> = from.iter().map(|each| each.to_string()).collect();

        let work = intent(&each.label, from, &each.description, &scenario.controller);
        let id = block_on(storage.submit_intent(&work)).expect("the intent is accepted");
        let id_text = id.to_string();
        block_on(storage.claim_intent(&id_text, &scenario.controller))
            .expect("the controller claims the intent");

        let body = if let Some(mut result) = each.scope2.clone() {
            result.source_fact = sources.first().cloned().expect("scope 2 reads a source");
            text(&result)
        } else if let Some(mut result) = each.scope1.clone() {
            result.source_facts = sources.clone();
            text(&result)
        } else if let Some(mut result) = each.intensity.clone() {
            result.source_facts = sources.clone();
            text(&result)
        } else {
            panic!("{} declares no result", each.label);
        };

        let concluded = block_on(storage.conclude_intent(&id_text, &body))
            .expect("the conclusion is recorded");
        written.insert(each.label.clone(), concluded.id);
    }

    // The requirements the results answer.
    for each in &scenario.requirements {
        block_on(storage.submit_hint(&Hint {
            id: CoordId::from_label(&each.label),
            content: each.content.clone(),
            creator: scenario.baseline.clone(),
        }))
        .expect("the requirement is recorded");
    }

    block_on(storage.flush_pending()).expect("every write reaches the medium");
}

/// Every record and payload a volume holds, by name.
///
/// A fact's name is its address, and the address is derived from what the record holds, so two volumes with the same
/// names hold the same records and the same payloads. The timestamp a record is filed with is the clock's rather than the
/// scenario's, which is why the comparison is by name.
fn entries(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for part in ["facts", "intents", "hints", "blob"] {
        let read = std::fs::read_dir(dir.join(part)).expect("the volume holds this part");
        for each in read {
            let name = each
                .expect("the volume is readable")
                .file_name()
                .into_string()
                .expect("a volume names its records in ASCII");
            if name.ends_with(".meta") {
                continue;
            }
            found.push(format!("{part}/{name}"));
        }
    }
    found.sort();
    found
}

#[test]
#[ignore = "authors the committed ESG volume rather than checking anything"]
fn author_the_esg_scenario() {
    let scenario = load();
    let dir = volume();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the volume directory is created");
    author(&dir, &scenario);
    println!("wrote {}", dir.display());
}

#[test]
fn the_scenario_file_makes_the_committed_records() {
    // The scenario is data, so what the gate checks is that the data still makes the volume this repository committed.
    // The data file is what a person edits, and this is the line that makes an edit land.
    let scenario = load();
    let scratch = tempfile::tempdir().expect("a scratch volume");
    author(scratch.path(), &scenario);

    let committed = volume();
    assert_eq!(
        entries(scratch.path()),
        entries(&committed),
        "the scenario makes the records and payloads the volume holds"
    );

    // The names carry what a fact holds, because a fact is addressed by its content. A transformation's declaration is
    // not a fact: the volume keeps it as the payload the intent points at, so it is checked as one, and a requirement's
    // text is held in the record itself.
    let payloads: Vec<Vec<u8>> = std::fs::read_dir(committed.join("blob"))
        .expect("the volume holds its payloads")
        .filter(|each| {
            each.as_ref()
                .map(|entry| entry.file_name().to_string_lossy().ends_with(".bin"))
                .unwrap_or(false)
        })
        .map(|each| std::fs::read(each.expect("the payload is readable").path()).expect("readable"))
        .collect();
    for each in &scenario.transformations {
        assert!(
            payloads.iter().any(|bytes| bytes == each.description.as_bytes()),
            "{} declares the description the data file states",
            each.label
        );
    }
    for each in &scenario.requirements {
        let path = committed
            .join("hints")
            .join(format!("h_{}.hint", CoordId::from_label(&each.label)));
        let bytes = std::fs::read(&path).expect("the volume holds this requirement");
        assert!(
            String::from_utf8_lossy(&bytes).contains(&each.content),
            "{} states the text the data file states",
            each.label
        );
    }
}
