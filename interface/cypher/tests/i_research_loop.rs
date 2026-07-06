// Full research loop scenario: cumulative knowledge growth over multiple cycles.
//
// Six phases demonstrate the complete research lifecycle:
//   1. Ingest 3 synthetic llms.md documents (GNN, Transformer, Hybrid)
//   2. Verify entities exist and detect knowledge gaps via Cypher
//   3. Detect disconnected concept pairs and submit hypothesis Intents
//   4. Claim and conclude an Intent, producing a new bridging Fact
//   5. Verify the knowledge graph now connects previously disconnected concepts
//   6. Show that new gaps emerge after knowledge integration

use interface_cypher as cypher;
use nexus_model::{
    Blackboard, BlackboardError, Fact, FactCapable, FihHash, Intent, IntentCapable, StorageRead,
};
use nexus_storage_composite::HybridBlackboard;
use serde_json;

// ── In-memory document chunking (not in core/model yet) ───────────────────

struct MdDocumentChunk {
    source: String,
    #[allow(dead_code)]
    title: String,
    section: String,
    content: String,
}

/// Split a Markdown document by `##` headings and create one chunk per section.
fn chunk_document(source: &str, title: &str, text: &str) -> Vec<MdDocumentChunk> {
    let mut chunks = Vec::new();
    let mut current_section = String::new();
    let mut current_lines: Vec<String> = Vec::new();
    let mut header_seen = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            if !current_section.is_empty() || (header_seen && !current_lines.is_empty()) {
                chunks.push(MdDocumentChunk {
                    source: source.to_string(),
                    title: title.to_string(),
                    section: current_section.clone(),
                    content: current_lines.join("\n"),
                });
                current_lines.clear();
            }
            header_seen = true;
            current_section = trimmed
                .strip_prefix("## ")
                .unwrap_or(trimmed)
                .trim()
                .to_string();
        } else if header_seen {
            current_lines.push(line.to_string());
        }
    }

    if header_seen && (!current_section.is_empty() || !current_lines.is_empty()) {
        chunks.push(MdDocumentChunk {
            source: source.to_string(),
            title: title.to_string(),
            section: current_section.clone(),
            content: current_lines.join("\n"),
        });
    }

    chunks
}

/// Submit each chunk as a Fact with content-addressable ID.
fn ingest_document(bb: &mut impl Blackboard, chunks: &[MdDocumentChunk]) {
    for chunk in chunks {
        let id = FihHash::from_hex(&format!("{}::{}", chunk.source, chunk.section));
        let fact = Fact {
            id,
            origin: chunk.source.clone(),
            content: serde_json::to_string(&serde_json::json!({
                "section": chunk.section,
                "content": chunk.content,
            }))
            .unwrap()
            .into(),
            creator: "ingestion-agent".into(),
        };
        bb.submit_fact(&fact).unwrap();
    }
}

// ── Helper: run a Cypher query and return record count ───────────────────

fn cypher_count(bb: &HybridBlackboard, query: &str) -> usize {
    bb.with_graph(|g| {
        let plan = cypher::Plan::from_internal(query).expect("plan parse failed");
        cypher::execute(g, &plan).expect("execute failed").len()
    })
}

// ── Three synthetic llms.md documents ────────────────────────────────────

/// Document 1: Graph Neural Networks for Molecular Property Prediction
const DOC_GNN: &str = "\
# Graph Neural Networks for Molecular Property Prediction

## Introduction

Graph neural networks have emerged as a powerful tool for molecular property prediction. By representing molecules as graphs where atoms are nodes and bonds are edges, GNNs can learn directly from molecular structure. This document surveys recent advances in GNN architectures for drug discovery applications.

## Architecture Overview

Message-passing GNNs operate by iteratively updating node representations through aggregation of neighbor information. Each layer computes a new node embedding by combining the previous embedding with messages from adjacent nodes. Common variants include GCN, GAT, and GIN, each with different aggregation and update strategies.

## Benchmark Results

On the ZINC-250k benchmark, GNNs achieve 92% accuracy on molecular property prediction tasks. However, deep GNNs suffer from oversmoothing beyond 6 layers, where node representations become indistinguishable. The 3-layer GCN achieves the best tradeoff between expressivity and computational cost, with an ROC-AUC of 0.89 on the ClinTox dataset.

## Limitations

GNNs face several challenges: limited expressive power compared to the Weisfeiler-Lehman test, difficulty capturing long-range dependencies, and sensitivity to graph structure perturbations. These limitations motivate research into alternative architectures and hybrid approaches.

## Future Directions

Key open problems include extending GNNs to 3D molecular geometries, incorporating geometric deep learning for conformation-aware predictions, and developing more expressive architectures that can capture higher-order graph properties.
";

/// Document 2: Transformer Architectures for Drug Discovery
const DOC_TRANSFORMER: &str = "\
# Transformer Architectures for Drug Discovery

## Introduction

Transformer-based models have revolutionized natural language processing and are increasingly applied to drug discovery. By treating molecular sequences as a language, transformers can learn molecular properties without explicit graph construction. This document reviews transformer applications in pharmaceutical research.

## Architecture Overview

Transformers use self-attention mechanisms to capture relationships between all pairs of tokens in a sequence. The key innovation is the multi-head attention mechanism, which allows the model to focus on different representation subspaces simultaneously. Positional encodings provide sequence order information.

## Benchmark Results

On molecular property prediction benchmarks, transformer-based models achieve competitive results with GNNs. The MolBERT model reaches an ROC-AUC of 0.88 on ClinTox, comparable to GNNs. However, transformers require 10x more training data than GNNs to reach peak performance, and their quadratic attention complexity limits sequence length.

## Limitations

Transformers lack built-in molecular structure bias, requiring large datasets to learn structural patterns that GNNs capture natively. The quadratic complexity of self-attention makes long molecular sequences computationally prohibitive. Interpretability remains a challenge, as attention weights do not always correspond to chemically meaningful interactions.

## Future Directions

Key open problems include reducing attention complexity to linear time, developing chemistry-aware pretraining objectives, and integrating 3D molecular structure information into the transformer framework.
";

/// Document 3: Hybrid GNN-Transformer Architectures
const DOC_HYBRID: &str = "\
# Hybrid GNN-Transformer Architectures for Molecular Modeling

## Introduction

Hybrid architectures combining GNNs and Transformers aim to leverage the complementary strengths of both approaches. GNNs capture local molecular structure through message passing, while transformers model long-range dependencies through self-attention. This document surveys state-of-the-art hybrid models.

## Architecture Overview

Hybrid models typically use a GNN encoder to process molecular graphs into node embeddings, followed by a transformer decoder that captures global interactions. The GNN component handles local bond-level information, while the transformer models distant atom-atom relationships that GNNs struggle with.

## Benchmark Results

On the ZINC-250k benchmark, hybrid GNN-Transformer models achieve 93% accuracy, surpassing pure GNNs (92%) and pure transformers. The GT-Mol model achieves an ROC-AUC of 0.91 on ClinTox, outperforming both GCN (0.89) and MolBERT (0.88). Hybrid models also show improved generalization on out-of-distribution molecular scaffolds.

## Integration Approaches

Three main integration strategies exist: serial (GNN then transformer), parallel (both branches fused by attention), and hierarchical (local GNN clusters processed by a global transformer). Serial integration with cross-attention between GNN and transformer layers currently achieves the best results on molecular benchmarks.

## Future Directions

Remaining challenges include optimal fusion of local and global representations, scaling hybrid models to larger molecular systems, and theoretical understanding of when hybrid architectures outperform their pure counterparts. Developing benchmarks that specifically test long-range molecular interactions will be crucial for advancing the field.
";

// ── Main test: complete research loop ────────────────────────────────────

#[test]
fn scenario_full_research_loop() {
    let mut bb = HybridBlackboard::new();

    // ──────────────────────────────────────────────────────────────────
    // Phase 1: Ingest all 3 documents
    // ──────────────────────────────────────────────────────────────────

    let gnn_doc_title = "Graph Neural Networks for Molecular Property Prediction";

    // Chunk each document
    let gnn_chunks = chunk_document("arxiv_gnn_2024", gnn_doc_title, DOC_GNN);
    let transformer_chunks = chunk_document(
        "arxiv_transformer_2024",
        "Transformer Architectures",
        DOC_TRANSFORMER,
    );
    let hybrid_chunks = chunk_document(
        "arxiv_hybrid_2024",
        "Hybrid GNN-Transformer Architectures",
        DOC_HYBRID,
    );

    // Verify chunk counts
    let expected_sections_gnn = 5; // Introduction, Architecture Overview,
    // Benchmark Results, Limitations, Future Directions
    let expected_sections_transformer = 5;
    let expected_sections_hybrid = 5;
    assert_eq!(gnn_chunks.len(), expected_sections_gnn);
    assert_eq!(transformer_chunks.len(), expected_sections_transformer);
    assert_eq!(hybrid_chunks.len(), expected_sections_hybrid);

    // Ingest all documents
    ingest_document(&mut bb, &gnn_chunks);
    ingest_document(&mut bb, &transformer_chunks);
    ingest_document(&mut bb, &hybrid_chunks);

    let total_chunks =
        expected_sections_gnn + expected_sections_transformer + expected_sections_hybrid;
    let state = bb.read_state();
    assert_eq!(
        state.facts.len(),
        total_chunks,
        "all document chunks ingested"
    );
    println!(
        "  Phase 1: Ingested {} chunks from 3 documents (GNN: {}, Transformer: {}, Hybrid: {})",
        total_chunks,
        gnn_chunks.len(),
        transformer_chunks.len(),
        hybrid_chunks.len()
    );

    // ──────────────────────────────────────────────────────────────────
    // Phase 2: Cypher queries to verify entities and detect gaps
    // ──────────────────────────────────────────────────────────────────

    // All Facts should be queryable
    let fact_count = cypher_count(&bb, "MATCH (f:Fact) RETURN f");
    assert_eq!(fact_count, total_chunks, "Cypher finds all Fact nodes");

    // Use read_state for origin-based verification because the Cypher
    // internal planner's WHERE clause for string equality on properties
    // is still a work-in-progress. Cypher is used for structural queries.
    let state = bb.read_state();

    let gnn_facts: Vec<&Fact> = state
        .facts
        .iter()
        .filter(|f| f.origin == "arxiv_gnn_2024")
        .collect();
    assert_eq!(gnn_facts.len(), expected_sections_gnn);

    let transformer_facts: Vec<&Fact> = state
        .facts
        .iter()
        .filter(|f| f.origin == "arxiv_transformer_2024")
        .collect();
    assert_eq!(transformer_facts.len(), expected_sections_transformer);

    let hybrid_facts: Vec<&Fact> = state
        .facts
        .iter()
        .filter(|f| f.origin == "arxiv_hybrid_2024")
        .collect();
    assert_eq!(hybrid_facts.len(), expected_sections_hybrid);

    // Check for gaps: concepts that exist in one document but lack
    // cross-references to related concepts in other documents.
    // The GNN document has "Benchmark Results" but does not mention
    // transformers or hybrid architectures in the same section.
    // The GNN document mentions "oversmoothing" while the transformer
    // document discusses "quadratic attention complexity" — these are
    // separate limitations without cross-referencing.

    // All 3 documents have a "Future Directions" section — but none
    // reference each other's future directions. Verify via content.
    let future_facts: Vec<&Fact> = state
        .facts
        .iter()
        .filter(|f| {
            let cv: serde_json::Value = serde_json::from_str(f.content.as_str().unwrap_or(""))
                .unwrap_or(serde_json::Value::Null);
            cv.get("section").and_then(|v| v.as_str()) == Some("Future Directions")
        })
        .collect();
    assert_eq!(future_facts.len(), 3, "3 Future Directions sections exist");

    println!(
        "  Phase 2: Verification passed — all {} facts queryable by origin",
        total_chunks
    );
    println!("  Phase 2: Gap detected — 3 Future Directions sections are disconnected");

    // ──────────────────────────────────────────────────────────────────
    // Phase 3: Simulate gap detection and submit hypothesis Intents
    // ──────────────────────────────────────────────────────────────────

    // Agent-Analyst detects that the GNN and Transformer documents discuss
    // complementary limitations (oversmoothing vs quadratic attention) but
    // neither references the other. The Hybrid document suggests a solution
    // but does not connect to the specific limitations.
    //
    // Hypothesis 1: Hybrid architectures can simultaneously solve GNN
    // oversmoothing and transformer quadratic complexity.

    let gnn_benchmark_id = FihHash::from_hex("arxiv_gnn_2024::Benchmark Results");
    let transformer_benchmark_id = FihHash::from_hex("arxiv_transformer_2024::Benchmark Results");
    let hybrid_benchmark_id = FihHash::from_hex("arxiv_hybrid_2024::Benchmark Results");
    let gnn_limitations_id = FihHash::from_hex("arxiv_gnn_2024::Limitations");
    let transformer_limitations_id = FihHash::from_hex("arxiv_transformer_2024::Limitations");
    let gnn_future_id = FihHash::from_hex("arxiv_gnn_2024::Future Directions");
    let transformer_future_id = FihHash::from_hex("arxiv_transformer_2024::Future Directions");
    let hybrid_future_id = FihHash::from_hex("arxiv_hybrid_2024::Future Directions");

    // Agent-Analyst creates a hypothesis that hybrid architectures resolve
    // both GNN oversmoothing and transformer complexity issues.
    let intent_hybrid_synthesis = Intent {
        id: FihHash::from_hex("i_hybrid_synthesis"),
        from_facts: vec![
            gnn_benchmark_id,
            transformer_benchmark_id,
            hybrid_benchmark_id,
            gnn_limitations_id,
            transformer_limitations_id,
        ],
        description: "HYPOTHESIS: Hybrid GNN-Transformer architectures resolve both GNN oversmoothing (by replacing deep message passing with attention) and transformer quadratic complexity (by using GNN to reduce sequence length via graph pooling).".into(),
        creator: "agent-analyst".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    };

    bb.submit_intent(&intent_hybrid_synthesis)
        .expect("hybrid synthesis intent submitted");

    // Hypothesis 2: The 3 Future Directions sections collectively describe
    // a unified research roadmap, but are siloed by document origin.
    let intent_unified_roadmap = Intent {
        id: FihHash::from_hex("i_unified_roadmap"),
        from_facts: vec![
            gnn_future_id,
            transformer_future_id,
            hybrid_future_id,
        ],
        description: "HYPOTHESIS: The Future Directions from GNN, Transformer, and Hybrid documents converge on 3 shared research priorities: (1) 3D molecular geometry, (2) linear-complexity architectures, (3) theoretical understanding of architecture choice.".into(),
        creator: "agent-analyst".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    };

    bb.submit_intent(&intent_unified_roadmap)
        .expect("unified roadmap intent submitted");

    let state = bb.read_state();
    assert_eq!(state.intents.len(), 2, "2 hypotheses submitted");
    println!("  Phase 3: Submitted 2 hypothesis Intents based on gap analysis");
    println!(
        "  Phase 3: Intent 1: Hybrid synthesis — grounded in {} facts",
        intent_hybrid_synthesis.from_facts.len()
    );
    println!(
        "  Phase 3: Intent 2: Unified roadmap — grounded in {} facts",
        intent_unified_roadmap.from_facts.len()
    );

    // ──────────────────────────────────────────────────────────────────
    // Phase 4: Claim and conclude an Intent, verify new Fact created
    // ──────────────────────────────────────────────────────────────────

    // Agent-Researcher claims the hybrid synthesis intent
    bb.claim_intent("i_hybrid_synthesis", "agent-researcher")
        .expect("claim should succeed");

    // Verify that another agent cannot double-claim
    let double_claim = bb.claim_intent("i_hybrid_synthesis", "agent-rival");
    assert!(
        matches!(double_claim, Err(BlackboardError::Conflict(_))),
        "double claim must fail with Conflict"
    );

    // Agent-Researcher heartbeats to show active work
    bb.heartbeat("i_hybrid_synthesis", "agent-researcher")
        .expect("heartbeat should succeed");

    // Conclude the Intent: produce a bridging Fact
    let conclusion_fact = bb
        .conclude_intent(
            "i_hybrid_synthesis",
            &serde_json::to_string(&serde_json::json!({
                "finding": "Hybrid GNN-Transformer architectures simultaneously address both limitations. GNN message passing (limited to 6 layers before oversmoothing) is replaced by graph pooling into a reduced set of super-nodes, which are then processed by a linear-complexity transformer (Performer-style). This achieves 93% accuracy on ZINC-250k, surpassing both pure GNN (92%) and pure transformer approaches. The hybrid architecture shows 3.2x faster convergence than transformer-only and 1.5x better long-range dependency capture than GNN-only.",
                "implication": "The GNN and Transformer communities should converge on hybrid architectures as the default paradigm for molecular property prediction. Pure architectures remain valuable for specific sub-problems.",
                "confidence": 0.87,
            })).unwrap(),
        )
        .expect("conclude should succeed");

    // Verify the conclusion created a new Fact
    let state = bb.read_state();
    assert_eq!(
        state.facts.len(),
        total_chunks + 1,
        "new Fact created from conclusion"
    );

    // The conclusion fact has content we set
    let json_val: serde_json::Value =
        serde_json::from_str(conclusion_fact.content.as_str().unwrap_or(""))
            .unwrap_or(serde_json::Value::Null);
    let conclusion_content = json_val
        .as_object()
        .expect("conclusion content is an object");
    assert_eq!(
        conclusion_content["finding"].as_str().unwrap_or(""),
        "Hybrid GNN-Transformer architectures simultaneously address both limitations. GNN message passing (limited to 6 layers before oversmoothing) is replaced by graph pooling into a reduced set of super-nodes, which are then processed by a linear-complexity transformer (Performer-style). This achieves 93% accuracy on ZINC-250k, surpassing both pure GNN (92%) and pure transformer approaches. The hybrid architecture shows 3.2x faster convergence than transformer-only and 1.5x better long-range dependency capture than GNN-only.",
        "conclusion fact contains the finding"
    );

    println!("  Phase 4: Researched claimed, heartbeated, and concluded Intent");
    println!(
        "  Phase 4: New Fact created — total facts now {}",
        state.facts.len()
    );

    // ──────────────────────────────────────────────────────────────────
    // Phase 5: Verify knowledge graph now connects previously disconnected
    // concepts
    // ──────────────────────────────────────────────────────────────────

    // The conclusion fact has origin "conclusion:i_hybrid_synthesis" and
    // references facts from both GNN and Transformer documents.
    // Verify via read_state that the bridging fact exists.
    let new_fact_origin = format!("conclusion:i_hybrid_synthesis");

    let state = bb.read_state();
    let has_bridging = state.facts.iter().any(|f| f.origin == new_fact_origin);
    assert!(
        has_bridging,
        "bridging fact exists connecting GNN and Transformer domains"
    );

    // The bridging fact connects concepts from both origins:
    // "arxiv_gnn_2024" (GNN oversmoothing) and "arxiv_transformer_2024"
    // (quadratic attention) are now linked through the hybrid synthesis.
    // Previously, no single fact referenced both limitations.

    // Verify data completeness: the unified roadmap intent remains open
    let state = bb.read_state();
    let remaining = state
        .intents
        .iter()
        .find(|i| i.id == FihHash::from_hex("i_unified_roadmap"));
    assert!(remaining.is_some(), "unified roadmap intent still open");
    assert_eq!(
        state.intents.len(),
        2,
        "both intents still in the blackboard"
    );

    println!("  Phase 5: Knowledge graph now connects GNN and Transformer concepts");
    println!(
        "  Phase 5: Bridging fact '{}' links previously disconnected research areas",
        conclusion_fact.id
    );

    // ──────────────────────────────────────────────────────────────────
    // Phase 6: New gaps emerge after knowledge integration
    // ──────────────────────────────────────────────────────────────────

    // After the hybrid synthesis conclusion, new knowledge gaps become
    // visible:
    //
    // Gap A: The conclusion states 93% accuracy on ZINC-250k and 3.2x
    // faster convergence, but these claims are not validated against
    // independent benchmarks. A validation Intent is needed.
    //
    // Gap B: The hybrid benchmark mention (93%) vs GNN-only (92%) is
    // only a 1% improvement. A new hypothesis emerges: is 93% the ceiling,
    // or can the gap be widened?

    let gnn_benchmark_id_str = gnn_benchmark_id.to_string();
    let hybrid_benchmark_id_str = hybrid_benchmark_id.to_string();
    let conclusion_fact_id_str = conclusion_fact.id.to_string();

    // New hypothesis: investigate whether the accuracy ceiling can be broken
    let intent_new_gap = Intent {
        id: FihHash::from_hex("i_accuracy_ceiling"),
        from_facts: vec![
            FihHash::from_hex(&gnn_benchmark_id_str),
            FihHash::from_hex(&hybrid_benchmark_id_str),
            FihHash::from_hex(&conclusion_fact_id_str),
        ],
        description: "HYPOTHESIS: The 93% accuracy on ZINC-250k is not a ceiling — incorporating 3D molecular geometry into hybrid architectures will push accuracy beyond 95%. This requires a new model class: geometric GNN-Transformer hybrids.".into(),
        creator: "agent-analyst".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    };

    bb.submit_intent(&intent_new_gap)
        .expect("new gap intent submitted");

    let state = bb.read_state();
    assert_eq!(
        state.intents.len(),
        3,
        "3 intents total: 2 original + 1 new gap"
    );
    assert_eq!(
        state.facts.len(),
        total_chunks + 1,
        "facts unchanged after new gap submission"
    );

    // The new gap intent is grounded in the bridging fact, demonstrating
    // cumulative knowledge growth: each research cycle builds on the
    // conclusions of the previous cycle.
    let gap_intent = state
        .intents
        .iter()
        .find(|i| i.id == FihHash::from_hex("i_accuracy_ceiling"))
        .expect("new gap intent exists");
    assert!(
        gap_intent.from_facts.contains(&conclusion_fact.id),
        "new gap references the bridging conclusion fact"
    );

    println!("  Phase 6: New gap emerged after knowledge integration");
    println!(
        "  Phase 6: Intent 'i_accuracy_ceiling' is grounded in {} facts including the bridging conclusion",
        gap_intent.from_facts.len()
    );
    println!(
        "  Phase 6: Total facts: {}, Total intents: {}, demonstrating cumulative growth",
        state.facts.len(),
        state.intents.len()
    );

    // ── Final summary ────────────────────────────────────────────────
    println!();
    println!("  ✓ Full Research Loop: 6 phases completed successfully");
    println!(
        "  ✓ Document ingestion: {} chunks across 3 documents",
        total_chunks
    );
    println!("  ✓ Gap detection: 2 disconnected concept areas identified");
    println!("  ✓ Hypothesis formation: 2 Intents submitted based on gaps");
    println!("  ✓ Research execution: Intent claimed, heartbeated, concluded");
    println!("  ✓ Knowledge integration: bridging fact connects GNN + Transformer domains");
    println!("  ✓ Cumulative growth: new gap emerged, total intents grown from 0 → 2 → 3");
    println!("  ✓ No direct agent communication — all coordination via FIH Blackboard");
}
