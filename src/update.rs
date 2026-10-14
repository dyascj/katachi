//! Auto-update from GitHub releases. A background check at launch (and every few hours) asks
//! GitHub for the latest release; when it's newer, the tab strip shows "Update now". Installing
//! downloads this platform's build, checks its minisign signature against the key built into the
//! app, swaps it in place, and restarts. Downloads go through the system `curl`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const REPO: &str = "dyascj/katachi";
/// Release builds are signed with the matching secret key in CI (MINISIGN_SECRET_KEY).
const PUBLIC_KEY: &str = "RWQKsFj9GVe/++/rK/p9ZbUslsNeaRLqP/9YhouncFqcletCka6bA2RQ";

/// This platform's release asset.
const ASSET: &str = if cfg!(target_os = "macos") {
    "Katachi-macos.app.tar.gz"
} else if cfg!(windows) {
    "Katachi-windows.zip"
} else if cfg!(target_arch = "aarch64") {
    "Katachi-linux-aarch64.tar.gz"
} else {
    "Katachi-linux-x86_64.tar.gz"
};

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Idle,
    Available { version: String, url: String, sig: String },
    Installing(String),
    /// Installed; restart into it.
    Ready(String),
    Failed(String),
}

pub struct Updater {
    pub(crate) state: Arc<Mutex<State>>,
    seen: State,
    /// The window, so the background check can wake it when it finds an update.
    wake: Arc<Mutex<Option<egui::Context>>>,
}

impl Updater {
    /// Check now and every six hours, unless this is a development build.
    pub fn start() -> Self {
        let off = cfg!(debug_assertions) || cfg!(test) || std::env::var_os("KATACHI_NO_UPDATE").is_some() || !crate::settings::load().check_updates || managed().is_some();
        let u = Updater { state: Arc::new(Mutex::new(State::Idle)), seen: State::Idle, wake: Arc::default() };
        // Windows can't delete a running exe, so the last update's old copy goes now.
        if cfg!(windows) && let Ok(exe) = std::env::current_exe() {
            let _ = std::fs::remove_file(exe.with_file_name("katachi.old.exe"));
        }
        if !off {
            let (state, wake) = (u.state.clone(), u.wake.clone());
            std::thread::spawn(move || loop {
                if let Ok(Some(found)) = check(env!("CARGO_PKG_VERSION")) {
                    let mut s = state.lock().unwrap();
                    if matches!(*s, State::Idle | State::Available { .. } | State::Failed(_)) {
                        *s = found;
                    }
                    if let Some(ctx) = wake.lock().unwrap().as_ref() {
                        ctx.request_repaint();
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(6 * 3600));
            });
        }
        u
    }

    #[cfg(test)]
    pub fn off() -> Self {
        Updater { state: Arc::new(Mutex::new(State::Idle)), seen: State::Idle, wake: Arc::default() }
    }

    /// Pick up the worker's state; true when it changed (or a restart is due).
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        {
            let mut w = self.wake.lock().unwrap();
            if w.is_none() {
                *w = Some(ctx.clone());
            }
        }
        let now = self.state.lock().unwrap().clone();
        let changed = now != self.seen;
        if changed && let State::Ready(_) = &now {
            restart();
        }
        self.seen = now;
        changed
    }

    pub fn button_label(&self) -> Option<&'static str> {
        match &self.seen {
            State::Available { .. } => Some("Update now"),
            State::Installing(_) => Some("Updating…"),
            State::Ready(_) => Some("Restarting…"),
            State::Failed(_) => Some("Update failed"),
            State::Idle => None,
        }
    }

    pub fn busy(&self) -> bool {
        matches!(self.seen, State::Installing(_) | State::Ready(_))
    }

    pub fn tooltip(&self) -> String {
        match &self.seen {
            State::Available { version, .. } => format!("Katachi {version} is available. Installs and restarts."),
            State::Installing(v) => format!("Installing Katachi {v}"),
            State::Ready(v) => format!("Katachi {v} installed"),
            State::Failed(e) => format!("{e}\nClick to try again."),
            State::Idle => String::new(),
        }
    }

    /// Download, verify and install the available version, then restart.
    pub fn install(&mut self) {
        let mut s = self.state.lock().unwrap();
        let (version, url, sig) = match &*s {
            State::Available { version, url, sig } => (version.clone(), url.clone(), sig.clone()),
            // After a failure, look again (the release may have been fixed).
            State::Failed(_) => {
                let state = self.state.clone();
                *s = State::Idle;
                std::thread::spawn(move || {
                    let found = check(env!("CARGO_PKG_VERSION")).unwrap_or_else(|e| Some(State::Failed(e)));
                    *state.lock().unwrap() = found.unwrap_or(State::Idle);
                });
                return;
            }
            _ => return,
        };
        *s = State::Installing(version.clone());
        drop(s);
        let (state, wake) = (self.state.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let done = install(&url, &sig).map_or_else(|e| State::Failed(format!("Couldn't update: {e}")), |()| State::Ready(version));
            *state.lock().unwrap() = done;
            if let Some(ctx) = wake.lock().unwrap().as_ref() {
                ctx.request_repaint();
            }
        });
    }
}

/// Why this copy can't update itself, when something else installs updates: a package manager
/// (installed under /usr), an AppImage or Flatpak (read-only), or a folder this user can't write.
pub fn managed() -> Option<&'static str> {
    static M: std::sync::OnceLock<Option<&'static str>> = std::sync::OnceLock::new();
    *M.get_or_init(managed_now)
}

fn managed_now() -> Option<&'static str> {
    if cfg!(target_os = "macos") || cfg!(windows) {
        return None;
    }
    if std::env::var_os("APPIMAGE").is_some() {
        return Some("This AppImage is updated by downloading the new one.");
    }
    if std::env::var_os("FLATPAK_ID").is_some() || std::env::var_os("SNAP").is_some() {
        return Some("Updates come from your software store.");
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let probe = dir.join(".katachi-write-test");
    if exe.starts_with("/usr") || std::fs::write(&probe, b"").is_err() {
        return Some("Updates come from your package manager.");
    }
    let _ = std::fs::remove_file(probe);
    None
}

/// Ask GitHub for the latest release; Some when it's newer than `current` and has this platform's build.
fn check(current: &str) -> Result<Option<State>, String> {
    let body = curl_text(&format!("https://api.github.com/repos/{REPO}/releases/latest"))?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().unwrap_or_default();
    if !newer(tag, current) {
        return Ok(None);
    }
    let url_of = |name: &str| {
        v["assets"].as_array().and_then(|a| a.iter().find(|x| x["name"] == name)).and_then(|x| x["browser_download_url"].as_str()).map(String::from)
    };
    Ok(match (url_of(ASSET), url_of(&format!("{ASSET}.minisig"))) {
        (Some(url), Some(sig)) => Some(State::Available { version: tag.trim_start_matches('v').to_string(), url, sig }),
        _ => None,
    })
}

/// Whether release tag `tag` ("v0.2.0") is a later version than `current` ("0.1.0").
pub fn newer(tag: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> { s.trim_start_matches('v').split(['.', '-', '+']).map_while(|p| p.parse().ok()).collect() };
    let (a, b) = (parse(tag), parse(current));
    !a.is_empty() && a > b
}

fn curl(args: &[&str]) -> Result<Vec<u8>, String> {
    let out = match std::process::Command::new("curl").args(["-fsSL", "--retry", "2", "-H", "User-Agent: Katachi"]).args(args).output() {
        // Some Linux installs ship wget but not curl.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !cfg!(windows) => {
            std::process::Command::new("wget").args(["-qO-", "--tries=3", "--header=User-Agent: Katachi"]).args(args).output().map_err(|_| "neither curl nor wget is installed".to_string())?
        }
        r => r.map_err(|e| format!("curl: {e}"))?,
    };
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(out.stdout)
}

fn curl_text(url: &str) -> Result<String, String> {
    curl(&[url]).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Whether `sig` (a .minisig file's text) is a valid signature of `data` by the release key.
pub fn verified(data: &[u8], sig: &str) -> bool {
    let Ok(pk) = minisign_verify::PublicKey::from_base64(PUBLIC_KEY) else { return false };
    let Ok(sig) = minisign_verify::Signature::decode(sig) else { return false };
    pk.verify(data, &sig, false).is_ok()
}

fn install(url: &str, sig_url: &str) -> Result<(), String> {
    let data = curl(&[url])?;
    let sig = curl_text(sig_url)?;
    if !verified(&data, &sig) {
        return Err("the download's signature doesn't match".into());
    }
    let target = install_target()?;
    let parent = target.parent().ok_or("no install folder")?;
    // Unpack next to the install, so the swap is a rename on one disk.
    let stage = parent.join(".katachi-update");
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).map_err(|e| format!("{}: {e}", stage.display()))?;
    let archive = stage.join(ASSET);
    std::fs::write(&archive, &data).map_err(|e| e.to_string())?;
    let status = std::process::Command::new("tar").arg(if ASSET.ends_with(".zip") { "-xf" } else { "-xzf" }).arg(&archive).arg("-C").arg(&stage).status().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("couldn't unpack the download".into());
    }
    let fresh = if cfg!(target_os = "macos") {
        stage.join("Katachi.app")
    } else if cfg!(windows) {
        stage.join("katachi.exe")
    } else {
        stage.join("katachi/katachi")
    };
    if !fresh.exists() {
        return Err("the download is missing the app".into());
    }
    // Never swap in a build this machine can't run (another CPU, or a newer system library).
    if !cfg!(target_os = "macos") {
        let ran = std::process::Command::new(&fresh).arg("--version").output();
        if !ran.as_ref().is_ok_and(|o| o.status.success() && o.stdout.starts_with(b"katachi ")) {
            let _ = std::fs::remove_dir_all(&stage);
            return Err("the new version doesn't run on this system".into());
        }
    }
    // Move the running copy aside (allowed even while it runs), put the new one in its place.
    let old = parent.join(if cfg!(target_os = "macos") { ".Katachi-old.app" } else if cfg!(windows) { "katachi.old.exe" } else { ".katachi-old" });
    let _ = remove(&old);
    std::fs::rename(&target, &old).map_err(|e| format!("{}: {e}", target.display()))?;
    if let Err(e) = std::fs::rename(&fresh, &target) {
        let _ = std::fs::rename(&old, &target);
        return Err(e.to_string());
    }
    let _ = remove(&old);
    let _ = std::fs::remove_dir_all(&stage);
    Ok(())
}

fn remove(p: &Path) -> std::io::Result<()> {
    if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) }
}

/// What gets replaced: the .app bundle on macOS, the executable elsewhere.
fn install_target() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if cfg!(target_os = "macos") {
        let bundle = exe.ancestors().nth(3).ok_or("not running from an app bundle")?;
        if bundle.extension().is_none_or(|e| e != "app") {
            return Err("not running from Katachi.app".into());
        }
        Ok(bundle.to_path_buf())
    } else {
        Ok(exe)
    }
}

/// Start the new version once this one has quit, then quit.
fn restart() {
    let Ok(target) = install_target() else { return };
    let pid = std::process::id();
    let t = target.display().to_string().replace('\'', "'\\''");
    let spawned = if cfg!(windows) {
        std::process::Command::new("cmd").args(["/C", &format!("timeout /T 2 /NOBREAK >NUL & start \"\" \"{}\"", target.display())]).spawn()
    } else {
        let launch = if cfg!(target_os = "macos") { format!("open '{t}'") } else { format!("'{t}' >/dev/null 2>&1 &") };
        std::process::Command::new("sh").args(["-c", &format!("while kill -0 {pid} 2>/dev/null; do sleep 0.2; done; {launch}")]).spawn()
    };
    if spawned.is_ok() {
        // Everything was saved before the update started, so this quits straight away.
        std::process::exit(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cargo test --release live_release -- --ignored`: the latest GitHub release offers this
    /// platform's build, and its download matches its signature.
    #[test]
    #[ignore]
    fn live_release_is_signed() {
        let Some(State::Available { version, url, sig }) = check("0.0.1").unwrap() else { panic!("no release with a signed {ASSET}") };
        let data = curl(&[&url]).unwrap();
        assert!(verified(&data, &curl_text(&sig).unwrap()), "{version}: {ASSET} doesn't match its signature");
        eprintln!("Katachi {version}: {ASSET} ({} MB) verified", data.len() >> 20);
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("v0.10.0", "0.9.3"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("v0.1.0", "0.2.0"));
        assert!(!newer("nightly", "0.1.0"));
    }

    #[test]
    fn only_release_signed_downloads_install() {
        // Signed with the release key.
        let sig = "untrusted comment: signature from minisign secret key\nRUQKsFj9GVe/+3EEA6Zds7Ncw7J2OBBHvXGvAHELYzKbel7k//o880elLYk6u4oirUkDuaExd6f6Lz5wrT43umXI925K9GudQgY=\ntrusted comment: test\ndi06iVvvVOGA1NkFmpibCA7q+GPrDziuu/mBhwf1KgNUtzqpitYS+XqJCF6HMpocXjQtIV2p+J8fWTotP5XEDg==\n";
        assert!(verified(b"hello katachi\n", sig));
        assert!(!verified(b"hello katachi!\n", sig), "a changed download fails");
        assert!(!verified(b"hello katachi\n", "garbage"));
    }
}
