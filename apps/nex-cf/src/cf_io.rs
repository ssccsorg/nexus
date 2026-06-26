// ── CfFihIo: Cloudflare R2-backed FileIo + BatchIo implementation ────
use nex::io::{BatchIo, FileIo, IoFuture, WriteOp};
use nex::storage::semantic::Query;
use worker::Bucket;

pub struct CfFihIo {
    bucket: Bucket,
}

impl CfFihIo {
    pub fn new(bucket: Bucket) -> Self {
        Self { bucket }
    }
}

impl FileIo for CfFihIo {
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
                .put(path, worker::Data::Bytes(data.to_vec()))
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
}

impl BatchIo for CfFihIo {
    fn apply_batch<'a>(&'a self, ops: &'a [WriteOp]) -> IoFuture<'a, ()> {
        Box::pin(async move {
            // Fire all R2 operations concurrently using spawn_local.
            // WASM single-thread: JS event loop processes all HTTP requests in parallel.
            let mut futs: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>> =
                Vec::new();
            for op in ops {
                let b = self.bucket.clone();
                match op {
                    WriteOp::Write { path, data } => {
                        let p = path.clone();
                        let v = data.clone();
                        futs.push(Box::pin(async move {
                            if let Err(msg) = b
                                .put(&p, worker::Data::Bytes(v))
                                .execute()
                                .await
                                .map_err(|m| format!("R2 put {p}: {m}"))
                            {
                                worker::console_log!("{msg}");
                            }
                        }));
                    }
                    WriteOp::Delete { path } => {
                        let p = path.clone();
                        futs.push(Box::pin(async move {
                            if let Err(msg) = b
                                .delete(&p)
                                .await
                                .map_err(|m| format!("R2 delete {p}: {m}"))
                            {
                                worker::console_log!("{msg}");
                            }
                        }));
                    }
                }
            }
            // Concurrently await all. In WASM, `join_all` on `select_all` runs in parallel
            // because each Future is backed by a JS Promise.
            for fut in futs {
                fut.await;
            }
            Ok(())
        })
    }
}

// ── TextQuery ──────────────────────────────────────────────────────
pub struct TextQuery {
    pub text: String,
}
impl Query for TextQuery {
    fn features(&self) -> Option<Vec<f32>> {
        None
    }
    fn text(&self) -> Option<String> {
        Some(self.text.clone())
    }
}
