// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Taeho Lee

// ── Clock abstraction ───────────────────────────────────────────────────

/// Clock abstraction for platform-independent timestamp generation.
pub trait Now {
    fn now_nanos(&self) -> u64;
    fn now_secs(&self) -> u64;
}
