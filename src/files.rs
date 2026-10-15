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

/// Write via a temp file and rename, so a crash or power loss mid-write never leaves a
/// half-written file. A symlink stays a symlink (its target is written), and the file keeps
/// its permissions.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let is_link = std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
    let path = if is_link { std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()) } else { path.to_path_buf() };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // The whole name plus a suffix, so it can't be a real sibling ("a.json" vs "a.katachi").
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".katachi-tmp");
    let tmp = path.with_file_name(name);
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        if let Ok(m) = std::fs::metadata(&path) {
            f.set_permissions(m.permissions())?;
        }
        f.sync_all()
    })();
    match written.and_then(|()| std::fs::rename(&tmp, &path)) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Autosaves left behind by sessions that didn't close cleanly (not another window's live one).
pub fn orphaned_recoveries(own: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(recovery_dir()) else { return vec![] };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "katachi") && p != own)
        .filter(|p| {
            // Names start with the writer's process id.
            let pid = p.file_name().and_then(|n| n.to_str()?.split('-').next()?.parse::<u32>().ok());
            let fresh = || std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age.as_secs() < 120);
            !pid.and_then(running).unwrap_or_else(fresh)
        })
        .collect();
    v.sort();
    v
}

/// Whether process `pid` is running, if that can be told.
fn running(pid: u32) -> Option<bool> {
    if pid == std::process::id() {
        return Some(true);
    }
    if cfg!(target_os = "linux") && Path::new("/proc/self").exists() {
        return Some(Path::new(&format!("/proc/{pid}")).exists());
    }
    #[cfg(windows)]
    let found = {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashing up.
        let out = std::process::Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/NH"]).creation_flags(0x0800_0000).output().ok()?;
        Some(String::from_utf8_lossy(&out.stdout).split_whitespace().any(|w| w == pid.to_string()))
    };
    #[cfg(not(windows))]
    let found = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .ok()
        .map(|s| s.success());
    found
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
    list.truncate(40);
    if let Ok(json) = serde_json::to_string(&list) {
        let _ = write_atomic(&data_dir().join("recent.json"), json.as_bytes());
    }
}

pub fn forget_recent(p: &Path) {
    let mut list = recent();
    list.retain(|q| q != p);
    if let Ok(json) = serde_json::to_string(&list) {
        let _ = write_atomic(&data_dir().join("recent.json"), json.as_bytes());
    }
}

/// Where Home keeps a file's thumbnail.
pub fn thumb_path(file: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut h);
    data_dir().join("thumbs").join(format!("{:016x}.png", h.finish()))
}

/// The file's version folder, named by an FNV-1a hash of its path (stable across Rust releases).
fn history_dir(file: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let fnv = file.as_os_str().as_encoded_bytes().iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3));
    let dir = data_dir().join("history").join(format!("{fnv:016x}"));
    // Earlier versions used DefaultHasher, whose output may change between Rust releases: move that folder over.
    let mut h = std::hash::DefaultHasher::new();
    file.hash(&mut h);
    let old = data_dir().join("history").join(format!("{:016x}", h.finish()));
    if !dir.exists() && old.is_dir() {
        let _ = std::fs::rename(&old, &dir);
    }
    dir
}

/// Keep a copy of each save (the newest 30) for version history.
pub fn save_version(file: &Path, bytes: &[u8]) {
    // Saving without changes doesn't make a new version.
    if versions(file).first().and_then(|(p, _)| std::fs::read(p).ok()).is_some_and(|b| b == bytes) {
        return;
    }
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
