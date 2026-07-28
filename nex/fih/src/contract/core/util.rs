// ── Shared contract utilities ──────────────────────────────────────────

/// Format a byte slice as a lowercase hex string.
/// Shared by GovernanceGate and EvidenceChain to avoid duplication.
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}
