//! App data on disk: atomic writes, crash-recovery autosaves, and recent files.

use std::path::{Path, PathBuf};

/// Per-user data directory (`KATACHI_DATA_DIR` overrides, e.g. for tests).
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("KATACHI_DATA_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Katachi")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(home).join("Katachi")
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share")).join("katachi")
    }
}

pub fn recovery_dir() -> PathBuf {
    data_dir().join("recovery")
}

/// Write via a temp file and rename, so a crash mid-write never leaves a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("katachi-tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Autosaves left behind by sessions that didn't close cleanly.
pub fn orphaned_recoveries(own: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(recovery_dir()) else { return vec![] };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "katachi") && p != own)
        .collect();
    v.sort();
    v
}

pub fn recent() -> Vec<PathBuf> {
    std::fs::read_to_string(data_dir().join("recent.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<PathBuf>>(&s).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.exists())
        .collect()
}

pub fn add_recent(p: &Path) {
    let mut list = recent();
    list.retain(|q| q != p);
    list.insert(0, p.to_path_buf());
    list.truncate(10);
    if let Ok(json) = serde_json::to_string(&list) {
        let _ = write_atomic(&data_dir().join("recent.json"), json.as_bytes());
    }
}

fn history_dir(file: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    file.hash(&mut h);
    data_dir().join("history").join(format!("{:016x}", h.finish()))
}

/// Keep a copy of each save (the newest 30) for version history.
pub fn save_version(file: &Path, bytes: &[u8]) {
    let dir = history_dir(file);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let _ = write_atomic(&dir.join(format!("{stamp}.katachi")), bytes);
    let mut all = versions(file);
    while all.len() > 30 {
        if let Some((oldest, _)) = all.pop() {
            let _ = std::fs::remove_file(oldest);
        }
    }
}

/// Saved versions of `file`, newest first, with their age in seconds.
pub fn versions(file: &Path) -> Vec<(PathBuf, u64)> {
    let Ok(rd) = std::fs::read_dir(history_dir(file)) else { return vec![] };
    let now = std::time::SystemTime::now();
    let mut v: Vec<(PathBuf, u64)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "katachi"))
        .map(|p| {
            let age = std::fs::metadata(&p).and_then(|m| m.modified()).ok().and_then(|t| now.duration_since(t).ok()).map_or(0, |d| d.as_secs());
            (p, age)
        })
        .collect();
    v.sort_by_key(|(p, _)| std::cmp::Reverse(p.clone()));
    v
}
