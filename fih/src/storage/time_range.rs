// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Taeho Lee

use crate::storage::read::StorageRead;
use alloc::string::String;
use core::ops::Range;

/// Backend reports its time coverage (for hot/cold routing).
pub trait TimeRangeCapable: StorageRead {
    fn time_range(&self) -> Option<Range<String>>;
}
