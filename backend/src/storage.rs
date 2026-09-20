use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_WRITE: AtomicU64 = AtomicU64::new(0);

/// Readers see either the previous complete file or the new complete file.
pub fn write_json(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_bytes(path, &bytes)
}

pub fn write_bytes(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("file has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!(
        "tmp-{}-{}",
        std::process::id(),
        NEXT_WRITE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(Into::into)
}
