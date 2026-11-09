//! Quality-of-life tools, folder helpers, protocol registration and diagnostics.

use crate::config::Settings;
use crate::paths::{roblox_logs_dir, Paths};
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use sysinfo::{ProcessesToUpdate, System};

#[allow(dead_code)]
pub fn hidden(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

pub fn open_folder(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(windows)]
    let prog = "explorer";
    #[cfg(not(windows))]
    let prog = "xdg-open";
    Command::new(prog).arg(path).spawn().with_context(|| format!("opening {}", path.display()))?;
    Ok(())
}

pub fn roblox_running() -> bool {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All);
    let running = sys.processes().values().any(|p| {
        p.name().to_string_lossy().eq_ignore_ascii_case(crate::roblox::PLAYER_EXE)
    });
    running
}

pub fn kill_roblox() -> Result<String> {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All);
    let mut n = 0;
    for p in sys.processes().values() {
        if p.name().to_string_lossy().eq_ignore_ascii_case(crate::roblox::PLAYER_EXE) && p.kill() {
            n += 1;
        }
    }
    Ok(if n == 0 { "Roblox is not running.".into() } else { format!("Closed {n} Roblox process(es).") })
}

fn remove_contents(dir: &Path) -> usize {
    let mut n = 0;
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let path = e.path();
            let ok = if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                fs::remove_dir_all(&path).is_ok()
            } else {
                fs::remove_file(&path).is_ok()
            };
            if ok {
                n += 1;
            }
        }
    }
    n
}

pub fn clear_roblox_cache() -> Result<String> {
    if roblox_running() {
        return Err(anyhow!("Close Roblox first."));
    }
    let mut n = 0;
    if let Some(local) = dirs::data_local_dir() {
        n += remove_contents(&local.join("Temp").join("Roblox"));
        n += remove_contents(&local.join("Roblox").join("rbx-storage"));
    }
    Ok(format!("Cleared {n} cache item(s)."))
}

pub fn clear_roblox_logs() -> Result<String> {
    let dir = roblox_logs_dir().ok_or_else(|| anyhow!("cannot locate Roblox logs"))?;
    Ok(format!("Removed {} log file(s).", remove_contents(&dir)))
}

pub fn uninstall_client(p: &Paths) -> Result<String> {
    if roblox_running() {
        return Err(anyhow!("Close Roblox first."));
    }
    let _ = crate::mods::restore(p);
    let n = remove_contents(&p.versions());
    Ok(format!("Removed the installed client ({n} folder(s)). It will be downloaded again on the next launch."))
}

fn newest_roblox_log() -> Option<PathBuf> {
    let dir = roblox_logs_dir()?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for e in fs::read_dir(dir).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.ends_with(".log") {
            continue;
        }
        if let Ok(m) = e.metadata().and_then(|m| m.modified()) {
            if best.as_ref().map(|(t, _)| m > *t).unwrap_or(true) {
                best = Some((m, e.path()));
            }
        }
    }
    best.map(|(_, p)| p)
}

fn sensitive(line: &str) -> bool {
    let l = line.to_lowercase();
    ["ticket", "token", "roblosecurity", "authorization", "cookie", "password"].iter().any(|k| l.contains(k))
}

/// Writes a zip with logs and settings. Lines that look sensitive are dropped.
pub fn export_diagnostics(p: &Paths, s: &Settings) -> Result<PathBuf> {
    fs::create_dir_all(p.diagnostics())?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let out = p.diagnostics().join(format!("diagnostics-{stamp}.zip"));
    let file = fs::File::create(&out)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();

    let info = format!(
        "{} {}\nOS: {} / {}\nInstalled client: {:?} ({:?})\nRoblox running: {}\nData folder: {}\nTime: {}\n",
        crate::branding::APP_NAME,
        crate::branding::APP_VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH,
        s.installed_label,
        s.installed_version,
        roblox_running(),
        p.root.display(),
        chrono::Local::now().to_rfc3339(),
    );
    zip.start_file("system.txt", opts)?;
    zip.write_all(info.as_bytes())?;

    let mut shown = s.clone();
    shown.seen_news.clear();
    zip.start_file("settings.json", opts)?;
    zip.write_all(serde_json::to_string_pretty(&shown)?.as_bytes())?;

    if let Ok(mut entries) = fs::read_dir(p.logs()).map(|rd| rd.flatten().collect::<Vec<_>>()) {
        entries.sort_by_key(|e| e.file_name());
        for e in entries.into_iter().rev().take(2) {
            if let Ok(text) = fs::read_to_string(e.path()) {
                zip.start_file(format!("launcher-{}", e.file_name().to_string_lossy()), opts)?;
                zip.write_all(text.as_bytes())?;
            }
        }
    }

    if let Some(log) = newest_roblox_log() {
        if let Ok(bytes) = fs::read(&log) {
            let text = String::from_utf8_lossy(&bytes);
            let lines: Vec<&str> = text.lines().collect();
            let tail = &lines[lines.len().saturating_sub(3000)..];
            let cleaned: Vec<&str> = tail.iter().copied().filter(|l| !sensitive(l)).collect();
            zip.start_file("roblox-log-tail.txt", opts)?;
            zip.write_all(cleaned.join("\n").as_bytes())?;
        }
    }
    zip.finish()?;
    Ok(out)
}

// ---------------------------------------------------------------- protocol handler

#[cfg(windows)]
pub fn register(p: &Paths) -> Result<()> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let exe = std::env::current_exe()?;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let mut previous = serde_json::Map::new();
    for scheme in ["roblox-player", "roblox"] {
        let cmd_path = format!("Software\\Classes\\{scheme}\\shell\\open\\command");
        let prev: Option<String> = hkcu.open_subkey(&cmd_path).ok().and_then(|k| k.get_value("").ok());
        if let Some(v) = prev {
            if !v.contains(&exe.to_string_lossy().to_string()) {
                previous.insert(scheme.to_string(), serde_json::Value::String(v));
            }
        }
    }
    let saved_path = p.prev_handler();
    if !previous.is_empty() || !saved_path.exists() {
        fs::write(&saved_path, serde_json::to_string_pretty(&previous)?)?;
    }
    for scheme in ["roblox-player", "roblox"] {
        let (key, _) = hkcu.create_subkey(format!("Software\\Classes\\{scheme}"))?;
        key.set_value("", &format!("URL:{scheme} Protocol"))?;
        key.set_value("URL Protocol", &"")?;
        let (cmd, _) = hkcu.create_subkey(format!("Software\\Classes\\{scheme}\\shell\\open\\command"))?;
        cmd.set_value("", &format!("\"{}\" \"%1\"", exe.display()))?;
    }
    Ok(())
}

#[cfg(windows)]
pub fn unregister(p: &Paths) -> Result<()> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let saved: serde_json::Map<String, serde_json::Value> = fs::read_to_string(p.prev_handler())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    for scheme in ["roblox-player", "roblox"] {
        let cmd_path = format!("Software\\Classes\\{scheme}\\shell\\open\\command");
        match saved.get(scheme).and_then(|v| v.as_str()) {
            Some(prev) => {
                let (cmd, _) = hkcu.create_subkey(&cmd_path)?;
                cmd.set_value("", &prev.to_string())?;
            }
            None => {
                let _ = hkcu.delete_subkey_all(format!("Software\\Classes\\{scheme}"));
            }
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn register(_p: &Paths) -> Result<()> {
    Err(anyhow!("protocol registration is only available on Windows"))
}

#[cfg(not(windows))]
pub fn unregister(_p: &Paths) -> Result<()> {
    Err(anyhow!("protocol registration is only available on Windows"))
}
