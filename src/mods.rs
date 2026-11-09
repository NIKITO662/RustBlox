//! Mod packs: every folder in Mods/ is a pack that can be switched on or off.
//! Original files are backed up before a pack overwrites them and restored on the next launch.

use crate::paths::Paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

const ROOTS: [(&str, &str); 4] = [
    ("content", "content"),
    ("platformcontent", "PlatformContent"),
    ("extracontent", "ExtraContent"),
    ("shaders", "shaders"),
];

fn canonical_root(name: &str) -> Option<&'static str> {
    let l = name.to_lowercase();
    ROOTS.iter().find(|(k, _)| *k == l).map(|(_, v)| *v)
}

#[derive(Clone, Debug)]
pub struct Pack {
    pub name: String,
    pub files: usize,
    pub ignored: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct Applied {
    version_dir: String,
    files: Vec<AppliedFile>,
}

#[derive(Serialize, Deserialize)]
struct AppliedFile {
    rel: String,
    had_original: bool,
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let rel = format!("{prefix}/{name}");
        let ft = entry.file_type()?;
        if ft.is_dir() {
            walk(&entry.path(), &rel, out)?;
        } else if ft.is_file() {
            out.push((rel, entry.path()));
        }
    }
    Ok(())
}

fn collect(pack_dir: &Path) -> (Vec<(String, PathBuf)>, Vec<String>) {
    let mut files = Vec::new();
    let mut ignored = Vec::new();
    if let Ok(rd) = fs::read_dir(pack_dir) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            match (is_dir, canonical_root(&name)) {
                (true, Some(root)) => {
                    if let Err(e) = walk(&entry.path(), root, &mut files) {
                        warn!("reading {}: {e}", entry.path().display());
                    }
                }
                _ => ignored.push(name),
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    (files, ignored)
}

pub fn list_packs(p: &Paths) -> Vec<Pack> {
    let mut packs = Vec::new();
    if let Ok(rd) = fs::read_dir(p.mods()) {
        for entry in rd.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let (files, ignored) = collect(&entry.path());
            packs.push(Pack { name, files: files.len(), ignored });
        }
    }
    packs.sort_by_key(|p| p.name.to_lowercase());
    packs
}

fn valid_pack_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty()
        && n.len() <= 64
        && n != "."
        && n != ".."
        && !n.chars().any(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

pub fn create_pack(p: &Paths, name: &str) -> Result<()> {
    let name = name.trim();
    if !valid_pack_name(name) {
        anyhow::bail!("pack names cannot be empty or contain \\ / : * ? \" < > |");
    }
    let dir = p.mods().join(name);
    if dir.exists() {
        anyhow::bail!("a pack named '{name}' already exists");
    }
    for (_, root) in ROOTS {
        fs::create_dir_all(dir.join(root))?;
    }
    Ok(())
}

pub fn delete_pack(p: &Paths, name: &str) -> Result<()> {
    if !valid_pack_name(name) {
        anyhow::bail!("invalid pack name");
    }
    fs::remove_dir_all(p.mods().join(name)).with_context(|| format!("deleting pack '{name}'"))
}

/// Imports a .zip as a new pack. Returns the pack name.
pub fn import_zip(p: &Paths, zip_path: &Path) -> Result<String> {
    let data = fs::read(zip_path).with_context(|| format!("reading {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(io::Cursor::new(data)).context("not a valid .zip file")?;

    let base = zip_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "imported".into());
    let base = if valid_pack_name(&base) { base } else { "imported".to_string() };
    let mut name = base.clone();
    let mut n = 2;
    while p.mods().join(&name).exists() {
        name = format!("{base}_{n}");
        n += 1;
    }

    // Strip a single wrapper folder (e.g. "MyMod/content/...") if it is not itself a mod root.
    let mut tops: HashSet<String> = HashSet::new();
    for i in 0..archive.len() {
        if let Some(path) = archive.by_index(i)?.enclosed_name() {
            if let Some(first) = path.components().next() {
                tops.insert(first.as_os_str().to_string_lossy().to_string());
            }
        }
    }
    let strip = if tops.len() == 1 {
        let only = tops.into_iter().next().unwrap_or_default();
        if canonical_root(&only).is_none() { Some(only) } else { None }
    } else {
        None
    };

    let dest = p.mods().join(&name);
    fs::create_dir_all(&dest)?;
    let mut count = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let rel = match entry.enclosed_name() {
            Some(r) => r.to_owned(),
            None => continue,
        };
        let rel = match &strip {
            Some(s) => match rel.strip_prefix(s) {
                Ok(r) => r.to_owned(),
                Err(_) => rel,
            },
            None => rel,
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut f = fs::File::create(&out)?;
            io::copy(&mut entry, &mut f)?;
            count += 1;
        }
    }
    if count == 0 {
        let _ = fs::remove_dir_all(&dest);
        anyhow::bail!("the zip was empty");
    }
    info!("imported pack '{name}' ({count} files)");
    Ok(name)
}

/// Undoes whatever the previous launch applied.
pub fn restore(p: &Paths) -> Result<usize> {
    let path = p.applied();
    if !path.exists() {
        return Ok(0);
    }
    let applied: Applied = fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let dir = PathBuf::from(&applied.version_dir);
    let mut n = 0;
    if dir.exists() {
        for f in &applied.files {
            let target = dir.join(&f.rel);
            if f.had_original {
                let backup = p.backups().join(&f.rel);
                if backup.exists() {
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::copy(&backup, &target)?;
                    n += 1;
                }
            } else if fs::remove_file(&target).is_ok() {
                n += 1;
            }
        }
    }
    let _ = fs::remove_dir_all(p.backups());
    fs::create_dir_all(p.backups())?;
    let _ = fs::remove_file(path);
    Ok(n)
}

fn save_applied(p: &Paths, applied: &Applied) -> Result<()> {
    fs::write(p.applied(), serde_json::to_string_pretty(applied)?)?;
    Ok(())
}

/// Applies every enabled pack (alphabetical order, later packs win).
pub fn apply(p: &Paths, dir: &Path, disabled: &BTreeSet<String>) -> Result<usize> {
    let mut applied = Applied { version_dir: dir.to_string_lossy().to_string(), files: Vec::new() };
    let mut seen: HashSet<String> = HashSet::new();
    let mut count = 0usize;

    let result: Result<()> = (|| {
        for pack in list_packs(p) {
            if disabled.contains(&pack.name) {
                continue;
            }
            let (items, _) = collect(&p.mods().join(&pack.name));
            for (rel, src) in items {
                let target = dir.join(&rel);
                if seen.insert(rel.clone()) {
                    let had = target.exists();
                    if had {
                        let backup = p.backups().join(&rel);
                        if let Some(parent) = backup.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::copy(&target, &backup)?;
                    }
                    applied.files.push(AppliedFile { rel: rel.clone(), had_original: had });
                    save_applied(p, &applied)?;
                }
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&src, &target).with_context(|| format!("copying {rel}"))?;
                count += 1;
            }
        }
        Ok(())
    })();
    save_applied(p, &applied)?;
    result?;
    if count > 0 {
        info!("applied {count} mod file(s)");
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_paths(tag: &str) -> Paths {
        let root = std::env::temp_dir().join(format!("rustblox-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for d in ["Mods", "Backups", "Versions"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        Paths { root }
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn apply_then_restore_returns_original_files() {
        let p = temp_paths("apply");
        let ver = p.root.join("Versions").join("version-x");
        write(&ver.join("content/a.txt"), "orig");
        write(&p.mods().join("01_base/content/a.txt"), "mod1");
        write(&p.mods().join("01_base/content/b.txt"), "new");
        write(&p.mods().join("02_over/Content/a.txt"), "mod2"); // case-insensitive root
        write(&p.mods().join("03_off/content/c.txt"), "never");
        write(&p.mods().join("01_base/readme.txt"), "ignored");

        let mut disabled = BTreeSet::new();
        disabled.insert("03_off".to_string());
        let n = apply(&p, &ver, &disabled).unwrap();
        assert_eq!(n, 3);
        assert_eq!(fs::read_to_string(ver.join("content/a.txt")).unwrap(), "mod2");
        assert_eq!(fs::read_to_string(ver.join("content/b.txt")).unwrap(), "new");
        assert!(!ver.join("content/c.txt").exists());
        assert!(!ver.join("readme.txt").exists());

        restore(&p).unwrap();
        assert_eq!(fs::read_to_string(ver.join("content/a.txt")).unwrap(), "orig");
        assert!(!ver.join("content/b.txt").exists());
        assert_eq!(restore(&p).unwrap(), 0);
        let _ = fs::remove_dir_all(&p.root);
    }

    #[test]
    fn reapplying_without_restore_still_keeps_true_original() {
        let p = temp_paths("reapply");
        let ver = p.root.join("Versions").join("v");
        write(&ver.join("content/a.txt"), "orig");
        write(&p.mods().join("m/content/a.txt"), "mod");
        apply(&p, &ver, &BTreeSet::new()).unwrap();
        restore(&p).unwrap();
        apply(&p, &ver, &BTreeSet::new()).unwrap();
        restore(&p).unwrap();
        assert_eq!(fs::read_to_string(ver.join("content/a.txt")).unwrap(), "orig");
        let _ = fs::remove_dir_all(&p.root);
    }

    #[test]
    fn zip_import_strips_wrapper_folder() {
        let p = temp_paths("zip");
        let zpath = p.root.join("CoolMod.zip");
        {
            let f = fs::File::create(&zpath).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("CoolMod/content/textures/x.png", o).unwrap();
            z.write_all(b"png").unwrap();
            z.finish().unwrap();
        }
        let name = import_zip(&p, &zpath).unwrap();
        assert_eq!(name, "CoolMod");
        assert!(p.mods().join("CoolMod/content/textures/x.png").exists());
        let packs = list_packs(&p);
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].files, 1);
        let again = import_zip(&p, &zpath).unwrap();
        assert_eq!(again, "CoolMod_2");
        let _ = fs::remove_dir_all(&p.root);
    }

    #[test]
    fn pack_names_are_validated() {
        let p = temp_paths("names");
        assert!(create_pack(&p, "../evil").is_err());
        assert!(create_pack(&p, "  ").is_err());
        assert!(create_pack(&p, "Good Pack").is_ok());
        assert!(create_pack(&p, "Good Pack").is_err());
        assert!(delete_pack(&p, "..").is_err());
        let _ = fs::remove_dir_all(&p.root);
    }
}
