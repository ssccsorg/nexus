// The ESG scenario the demonstration reads, authored here so the volume can be rebuilt.
//
// The volume is committed data, so this is not part of the gate: it writes the bytes a person
// later commits. Run it deliberately:
//
//   cargo test -p nexus-storage-ref --test esg_scenario -- --ignored --nocapture
//
// It writes `storage/ref/demo/esg-volume`, replacing what is there.
//
// The scenario is one reporting period of a synthetic manufacturer, shaped the way a quarterly
// inventory is: four measured quantities, two scope calculations that report Scope 2 on both a
// location-based and a market-based basis, an intensity metric over the two results, and the
// disclosure requirement they answer. Every number below is a fixture, and the arithmetic in the
// transformation descriptions is the only claim the data makes.

use futures_executor::block_on;
use nex_fih::FihId20 as _;
use nex_fih::{
    AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, Content, CoordId, Fact, Hint, Intent,
};
use nex_io::fs_io::FsIo;
use nexus_storage_ref::RefFihStorage;
use serde::Serialize;
use std::path::PathBuf;

fn volume() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("demo/esg-volume")
}

/// A payload as the author wrote it.
///
/// The fields keep the order of the struct, and the screen reads a payload in the order it arrives: a record that leads
/// with its figure is a record whose author said the figure leads. Serializing an unordered map instead would leave the
/// order to the format and put `base_year` where the result belongs.
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
    unit: &'static str,
    metric: &'static str,
    period: &'static str,
    site: &'static str,
    reference: &'static str,
    quality: &'static str,
    source: &'static str,
}

/// A refrigerant top-up, which is a mass and a global warming potential rather than an energy reading.
#[derive(Serialize)]
struct Fugitive {
    value: f64,
    unit: &'static str,
    metric: &'static str,
    substance: &'static str,
    gwp: u16,
    gwp_source: &'static str,
    period: &'static str,
    site: &'static str,
    reference: &'static str,
    quality: &'static str,
    source: &'static str,
}

/// Scope 2 as ESRS E1-6 asks for it: the same energy on both bases, with the factors named.
#[derive(Serialize)]
struct Scope2<'a> {
    market_based_tco2e: f64,
    location_based_tco2e: f64,
    scope: u8,
    period: &'a str,
    consolidation: &'a str,
    base_year: u16,
    restated: bool,
    factor_market_kg_per_kwh: f64,
    factor_location_kg_per_kwh: f64,
    factor_source: &'a str,
    source_fact: &'a str,
}

/// Scope 1 split by the source it came from, because combustion and fugitive emissions are reported apart.
#[derive(Serialize)]
struct Scope1<'a> {
    tco2e: f64,
    stationary_combustion_tco2e: f64,
    fugitive_refrigerant_tco2e: f64,
    scope: u8,
    period: &'a str,
    consolidation: &'a str,
    gwp_source: &'a str,
    base_year: u16,
    restated: bool,
    source_facts: Vec<&'a str>,
}

/// The intensity metric, which is what a reader compares across periods and sites.
#[derive(Serialize)]
struct Intensity<'a> {
    kgco2e_per_unit: f64,
    total_tco2e: f64,
    denominator_units: u32,
    basis: &'a str,
    period: &'a str,
    consolidation: &'a str,
    source_facts: Vec<&'a str>,
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
    // One plant, one quarter. The meters, the service record, and the production system are the origins, so a reader can
    // ask who reported each number, on which reference, and when it was read.

    let electricity = block_on(storage.submit_fact(&Fact::new(
        "did:example:plant-li-1".into(),
        content(&Measured {
            value: 412_000.0,
            unit: "kWh",
            metric: "electricity_consumption",
            period: "2026-Q1",
            site: "plant-li-1",
            reference: "M17-2024-0431",
            quality: "measured",
            source: "meter:m17",
        }),
        "meter:m17".into(),
    )))
    .expect("the electricity reading is accepted");

    let gas = block_on(storage.submit_fact(&Fact::new(
        "did:example:plant-li-1".into(),
        content(&Measured {
            value: 96_400.0,
            unit: "kWh",
            metric: "natural_gas_consumption",
            period: "2026-Q1",
            site: "plant-li-1",
            reference: "G04-2023-0117",
            quality: "measured",
            source: "meter:g04",
        }),
        "meter:g04".into(),
    )))
    .expect("the gas reading is accepted");

    let refrigerant = block_on(storage.submit_fact(&Fact::new(
        "did:example:plant-li-1".into(),
        content(&Fugitive {
            value: 12.5,
            unit: "kg",
            metric: "refrigerant_top_up",
            substance: "R-410A",
            gwp: 2088,
            gwp_source: "IPCC AR5 GWP100",
            period: "2026-Q1",
            site: "plant-li-1",
            reference: "WO-2026-0142",
            quality: "measured",
            source: "service:contractor",
        }),
        "service:contractor".into(),
    )))
    .expect("the refrigerant record is accepted");

    let output = block_on(storage.submit_fact(&Fact::new(
        "did:example:plant-li-1".into(),
        content(&Measured {
            value: 18_400.0,
            unit: "assemblies",
            metric: "production_output",
            period: "2026-Q1",
            site: "plant-li-1",
            reference: "ERP-2026Q1",
            quality: "measured",
            source: "erp:production",
        }),
        "erp:production".into(),
    )))
    .expect("the output record is accepted");

    // The rest of the site's instruments, which are not meters of one kind: an inverter that generates, a flow meter on
    // the water main, and a weighbridge at the scrap bay. They are the same volume and the same read path, which is the
    // point of the demonstration.

    let _generation = block_on(storage.submit_fact(&Fact::new(
        "did:example:plant-li-1".into(),
        content(&Measured {
            value: 63_500.0,
            unit: "kWh",
            metric: "solar_generation",
            period: "2026-Q1",
            site: "plant-li-1",
            reference: "INV-2022-0009",
            quality: "measured",
            source: "inverter:pv1",
        }),
        "inverter:pv1".into(),
    )))
    .expect("the generation record is accepted");

    let _water = block_on(storage.submit_fact(&Fact::new(
        "did:example:plant-li-1".into(),
        content(&Measured {
            value: 1_240.0,
            unit: "m3",
            metric: "water_withdrawal",
            period: "2026-Q1",
            site: "plant-li-1",
            reference: "W02-2025-0031",
            quality: "measured",
            source: "meter:w02",
        }),
        "meter:w02".into(),
    )))
    .expect("the water record is accepted");

    let _scrap = block_on(storage.submit_fact(&Fact::new(
        "did:example:plant-li-1".into(),
        content(&Measured {
            value: 3.8,
            unit: "t",
            metric: "metal_scrap_sold",
            period: "2026-Q1",
            site: "plant-li-1",
            reference: "SC-2026-0033",
            quality: "measured",
            source: "scale:s01",
        }),
        "scale:s01".into(),
    )))
    .expect("the scrap record is accepted");

    // The transformations, claimed and concluded.
    //
    // A conclusion is a fact about the intent, so the result is filed at the intent's own conclusion address and the chain
    // from the sources to the result is the intent's `from_facts`.

    // 412,000 kWh at 0.2053 and at 0.1280 kgCO2e/kWh.
    let electricity_text = electricity.to_string();
    let scope2 = intent(
        "intent_scope2_2026q1",
        vec![electricity],
        "apply the 2026 residual-mix factor (0.2053 kgCO2e/kWh) and the EEA grid factor (0.1280) to the metered \
         electricity, and state Scope 2 on both bases for 2026-Q1",
    );
    let scope2_id = block_on(storage.submit_intent(&scope2)).expect("the intent is accepted");
    let scope2_text = scope2_id.to_string();
    block_on(storage.claim_intent(&scope2_text, "sustainability-controller"))
        .expect("the controller claims the intent");
    let scope2_fact = block_on(storage.conclude_intent(
        &scope2_text,
        &text(&Scope2 {
            market_based_tco2e: 84.58,
            location_based_tco2e: 52.74,
            scope: 2,
            period: "2026-Q1",
            consolidation: "operational control",
            base_year: 2024,
            restated: false,
            factor_market_kg_per_kwh: 0.2053,
            factor_location_kg_per_kwh: 0.1280,
            factor_source: "illustrative 2026 residual mix and EEA grid mix",
            source_fact: &electricity_text,
        }),
    ))
    .expect("the conclusion is recorded");

    // 96,400 kWh at 0.2016 kgCO2e/kWh, and 12.5 kg at GWP 2088.
    let gas_text = gas.to_string();
    let refrigerant_text = refrigerant.to_string();
    let scope1 = intent(
        "intent_scope1_2026q1",
        vec![gas, refrigerant],
        "convert stationary combustion (0.2016 kgCO2e/kWh) and the R-410A top-up (GWP 2088, AR5) to Scope 1 for \
         2026-Q1",
    );
    let scope1_id = block_on(storage.submit_intent(&scope1)).expect("the intent is accepted");
    let scope1_text = scope1_id.to_string();
    block_on(storage.claim_intent(&scope1_text, "sustainability-controller"))
        .expect("the controller claims the intent");
    let scope1_fact = block_on(storage.conclude_intent(
        &scope1_text,
        &text(&Scope1 {
            tco2e: 45.53,
            stationary_combustion_tco2e: 19.43,
            fugitive_refrigerant_tco2e: 26.10,
            scope: 1,
            period: "2026-Q1",
            consolidation: "operational control",
            gwp_source: "IPCC AR5 GWP100",
            base_year: 2024,
            restated: false,
            source_facts: vec![&gas_text, &refrigerant_text],
        }),
    ))
    .expect("the conclusion is recorded");

    // 130.11 tCO2e over 18,400 assemblies.
    let scope1_fact_text = scope1_fact.id.to_string();
    let scope2_fact_text = scope2_fact.id.to_string();
    let output_text = output.to_string();
    let intensity = intent(
        "intent_intensity_2026q1",
        vec![scope1_fact.id, scope2_fact.id, output],
        "divide the reported Scope 1 and Scope 2 total (market-based) by the quarter's output to state the intensity \
         per assembly",
    );
    let intensity_id = block_on(storage.submit_intent(&intensity)).expect("the intent is accepted");
    let intensity_text = intensity_id.to_string();
    block_on(storage.claim_intent(&intensity_text, "sustainability-controller"))
        .expect("the controller claims the intent");
    block_on(storage.conclude_intent(
        &intensity_text,
        &text(&Intensity {
            kgco2e_per_unit: 7.07,
            total_tco2e: 130.11,
            denominator_units: 18_400,
            basis: "market-based",
            period: "2026-Q1",
            consolidation: "operational control",
            source_facts: vec![&scope1_fact_text, &scope2_fact_text, &output_text],
        }),
    ))
    .expect("the conclusion is recorded");

    // The requirements the results answer.

    block_on(storage.submit_hint(&Hint {
        id: CoordId::from_label("esrs_e1_6"),
        content: "ESRS E1-6: disclose gross Scope 1 and Scope 2 GHG emissions separately, Scope 2 on both a \
                  location-based and a market-based basis, with the consolidation approach, the base year, and the \
                  conversion factors used"
            .into(),
        creator: "regulatory-baseline".into(),
    }))
    .expect("the emissions requirement is recorded");

    block_on(storage.submit_hint(&Hint {
        id: CoordId::from_label("esrs_e3_4"),
        content: "ESRS E3-4: disclose total water consumption in cubic metres, with withdrawal and discharge by \
                  source, for the reporting period"
            .into(),
        creator: "regulatory-baseline".into(),
    }))
    .expect("the water requirement is recorded");

    block_on(storage.submit_hint(&Hint {
        id: CoordId::from_label("esrs_e5_5"),
        content: "ESRS E5-5: disclose the total quantity of waste generated and the quantity diverted from disposal, \
                  by waste stream"
            .into(),
        creator: "regulatory-baseline".into(),
    }))
    .expect("the waste requirement is recorded");

    block_on(storage.flush_pending()).expect("every write reaches the medium");
    println!("wrote {}", dir.display());
}
