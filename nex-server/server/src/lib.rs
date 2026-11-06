// ── nex-server: the node ───────────────────────────────────────────────
//
// The library target is what the binary and its tests share: the methods a node answers.
// The binary adds the socket, the hub registration, and the process lifetime.

pub mod handler;
