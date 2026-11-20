// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Taeho Lee

// ── nex-server: the node ───────────────────────────────────────────────
//
// The library target is what the binary and its tests share: the methods a node answers.
// The binary adds the socket, the hub registration, and the process lifetime.

pub mod handler;
