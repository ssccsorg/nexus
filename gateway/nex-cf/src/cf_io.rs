// ── CfFihIo: Cloudflare R2-backed AsyncFileIo implementation ──────────
//
// Wraps worker::Bucket (R2 binding) behind the AsyncFileIo trait.
// Enables FihStorage to run in Cloudflare Workers, using R2 as the
// persistent key-value store.
//
// Key-space mapping:
//   facts/f_{id}.fact       → r2 object "facts/f_{id}.fact"
//   intents/i_{id}.intent   → r2 object "intents/i_{id}.intent"
//   hints/h_{id}.hint       → r2 object "hints/h_{id}.hint"
//   blob/{hash}.bin         → r2 object "blob/{hash}.bin"
//   blob/{hash}.bin.meta    → r2 object "blob/{hash}.bin.meta"
//   flush/{part}/cursor_{t}.chain → r2 object "flush/..."
//
// Limitations:
//   - R2 list-after-write is eventually consistent (may miss recent writes)

use nex::io::{AsyncFileIo, IoFuture, WriteOp};
use worker::{Bucket, Data};

pub struct CfFihIo {
    bucket: Bucket,
}

impl CfFihIo {
    pub fn new(bucket: Bucket) -> Self {
        Self { bucket }
    }
}

impl AsyncFileIo for CfFihIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        Box::pin(async move {
            let obj = self
                .bucket
                .get(path)
                .execute()
                .await
                .map_err(|e| format!("R2 get {path}: {e}"))?;
            match obj {
                Some(o) => {
                    let bytes = o
                        .body()
                        .ok_or_else(|| format!("R2 {path}: no body"))?
                        .bytes()
                        .await
                        .map_err(|e| format!("R2 read {path}: {e}"))?;
                    Ok(Some(bytes))
                }
                None => Ok(None),
            }
        })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        Box::pin(async move {
            self.bucket
                .put(path, Data::Bytes(data.to_vec()))
                .execute()
                .await
                .map_err(|e| format!("R2 put {path}: {e}"))?;
            Ok(())
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        Box::pin(async move {
            let mut objects = self
                .bucket
                .list()
                .prefix(prefix)
                .execute()
                .await
                .map_err(|e| format!("R2 list {prefix}: {e}"))?;
            let mut keys = Vec::new();
            loop {
                for obj in objects.objects() {
                    keys.push(obj.key());
                }
                if !objects.truncated() {
                    break;
                }
                // SAFETY: cursor is present when truncated() is true
                let cursor = objects.cursor().unwrap();
                objects = self
                    .bucket
                    .list()
                    .prefix(prefix)
                    .cursor(cursor)
                    .execute()
                    .await
                    .map_err(|e| format!("R2 list next: {e}"))?;
            }
            Ok(keys)
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        Box::pin(async move {
            self.bucket
                .delete(path)
                .await
                .map_err(|e| format!("R2 delete {path}: {e}"))?;
            Ok(())
        })
    }

    fn apply_batch<'a>(&'a self, ops: &'a [WriteOp]) -> IoFuture<'a, ()> {
        Box::pin(async move {
            for op in ops {
                match op {
                    WriteOp::Write { path, data } => {
                        self.bucket
                            .put(path.as_str(), Data::Bytes(data.clone()))
                            .execute()
                            .await
                            .map_err(|e| format!("R2 put {path}: {e}"))?;
                    }
                    WriteOp::Delete { path } => {
                        self.bucket
                            .delete(path.as_str())
                            .await
                            .map_err(|e| format!("R2 delete {path}: {e}"))?;
                    }
                }
            }
            Ok(())
        })
    }
}
