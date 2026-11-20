// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Taeho Lee

use crate::storage::{FactCapable, HintCapable, IntentCapable, StorageRead};

// ── Blackboard trait — FIH lifecycle (public, stable) ─────────────────────
//
// Aggregate trait combining the four core storage capabilities.
// Any type implementing StorageRead + FactCapable + HintCapable + IntentCapable
// automatically satisfies Blackboard.

pub trait Blackboard: StorageRead + FactCapable + HintCapable + IntentCapable {}

impl<T: StorageRead + FactCapable + HintCapable + IntentCapable> Blackboard for T {}
