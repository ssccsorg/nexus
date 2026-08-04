// EntityStore key mapping: injective ByteWise fallback.
//
// The legacy byte-fold fallback conflated distinct keys (for example
// "  " and "wh" both mapped to the same path, because the two-byte
// pairs folded to the same coordinate modulo 11172). The fallback now
// uses the tagma-kv ByteWise mapping, one Coord per UTF-8 byte, which is
// injective for keys of at most N bytes. These tests pin that property
// through the CoordEntityStore surface.

use futures_executor::block_on;
use nex_fih::{CoordEntityStore, EntityStore};

#[test]
fn bytewise_fallback_distinguishes_legacy_colliding_keys() {
    block_on(async {
        let store = CoordEntityStore::<6, String>::new();
        // Under the old byte-fold these two keys mapped to one path.
        store.insert("  ".into(), "spaces".into()).await;
        store.insert("wh".into(), "letters".into()).await;
        assert_eq!(store.len().await, 2, "distinct keys must not collide");
        assert_eq!(store.get("  ").await.as_deref(), Some("spaces"));
        assert_eq!(store.get("wh").await.as_deref(), Some("letters"));
    });
}

#[test]
fn bytewise_fallback_injective_for_short_keys() {
    block_on(async {
        let store = CoordEntityStore::<6, String>::new();
        for (k, v) in [
            ("f_base", "a"),
            ("f_basd", "b"),
            ("f_basf", "c"),
            ("key-01", "d"),
        ] {
            store.insert(k.into(), v.into()).await;
        }
        assert_eq!(store.len().await, 4);
        assert_eq!(store.get("f_basd").await.as_deref(), Some("b"));
        assert_eq!(store.get("key-01").await.as_deref(), Some("d"));
    });
}

#[test]
fn hangul_fast_path_distinguishes_coordids() {
    block_on(async {
        let store = CoordEntityStore::<6, String>::new();
        store.insert("가나다라마바".into(), "first".into()).await;
        store.insert("가나다라마사".into(), "second".into()).await;
        assert_eq!(store.len().await, 2);
        assert_eq!(store.get("가나다라마사").await.as_deref(), Some("second"));
    });
}

#[test]
fn long_key_truncation_is_deterministic() {
    block_on(async {
        let store = CoordEntityStore::<6, String>::new();
        // Same key always maps to the same path: overwrite, not duplicate.
        store.insert("abcdefgh".into(), "first".into()).await;
        store.insert("abcdefgh".into(), "second".into()).await;
        assert_eq!(store.len().await, 1);
        assert_eq!(store.get("abcdefgh").await.as_deref(), Some("second"));

        // Keys longer than N bytes truncate at the first N bytes, so a
        // key sharing the prefix maps to the same path by design.
        store.insert("abcdefxy".into(), "third".into()).await;
        assert_eq!(store.len().await, 1);
        assert_eq!(store.get("abcdefgh").await.as_deref(), Some("third"));
    });
}
