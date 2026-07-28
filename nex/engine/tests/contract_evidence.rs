// ── EvidenceChain tests ───────────────────────────────────────────────

use nex::contract::EvidenceChain;

#[test]
fn test_new_chain_is_empty() {
    let chain = EvidenceChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
    assert!(chain.tip().is_none());
    assert!(chain.fingerprint().is_none());
}

#[test]
fn test_append_increases_length() {
    let chain = EvidenceChain::new();
    let h1 = chain.append("abc", "fact", 1000);
    assert_eq!(chain.len(), 1);
    assert_eq!(chain.tip(), Some(h1.clone()));

    let h2 = chain.append("def", "intent", 2000);
    assert_eq!(chain.len(), 2);
    assert_eq!(chain.tip(), Some(h2));
}

#[test]
fn test_entry_metadata() {
    let chain = EvidenceChain::new();
    chain.append("hash1", "fact", 5000);
    let e = chain.get(0).unwrap();
    assert_eq!(e.seq, 0);
    assert_eq!(e.action_hash, "hash1");
    assert_eq!(e.action_type, "fact");
    assert_eq!(e.timestamp_ns, 5000);
}

#[test]
fn test_chain_hashes_link() {
    let chain = EvidenceChain::new();
    let h1 = chain.append("a", "fact", 1000);
    let e1 = chain.get(0).unwrap();
    assert_eq!(e1.chain_hash, h1);

    let h2 = chain.append("b", "intent", 2000);
    let e2 = chain.get(1).unwrap();
    assert_eq!(e2.chain_hash, h2);
    assert_ne!(h1, h2);

    assert!(chain.verify(0));
}

#[test]
fn test_verify_empty_chain() {
    let chain = EvidenceChain::new();
    assert!(chain.verify(0));
    assert!(chain.verify(100));
}

#[test]
fn test_verify_from_mid_chain() {
    let chain = EvidenceChain::new();
    chain.append("a", "fact", 100);
    chain.append("b", "fact", 200);
    chain.append("c", "fact", 300);
    assert!(chain.verify(1));
    assert!(chain.verify(2));
    assert!(chain.verify(10));
}

#[test]
fn test_fingerprint_changes() {
    let chain = EvidenceChain::new();
    chain.append("a", "fact", 100);
    let fp1 = chain.fingerprint().unwrap();
    chain.append("b", "fact", 200);
    let fp2 = chain.fingerprint().unwrap();
    assert_ne!(fp1, fp2);
}

#[test]
fn test_get_out_of_range() {
    let chain = EvidenceChain::new();
    assert!(chain.get(0).is_none());
    chain.append("x", "fact", 1);
    assert!(chain.get(0).is_some());
    assert!(chain.get(1).is_none());
}

#[test]
fn test_entries_snapshot() {
    let chain = EvidenceChain::new();
    chain.append("1", "fact", 10);
    chain.append("2", "intent", 20);
    assert_eq!(chain.entries().len(), 2);
}

#[test]
fn test_tip_updates() {
    let chain = EvidenceChain::new();
    assert!(chain.tip().is_none());
    let h1 = chain.append("first", "fact", 1);
    assert_eq!(chain.tip().unwrap(), h1);
    let h2 = chain.append("second", "fact", 2);
    assert_eq!(chain.tip().unwrap(), h2);
    assert_ne!(h1, h2);
}

#[test]
fn test_consecutive_hashes_differ() {
    let chain = EvidenceChain::new();
    let hashes: Vec<_> = (0..5)
        .map(|i| chain.append(&format!("a{i}"), "fact", i * 100))
        .collect();
    let mut unique = hashes.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 5);
}
