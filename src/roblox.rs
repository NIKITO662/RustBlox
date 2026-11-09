//! Roblox client deployment: version lookup, install/update, launch arguments.

use crate::branding;
use crate::paths::Paths;
use anyhow::{anyhow, bail, Context, Result};
use md5::{Digest, Md5};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

pub const VERSION_URL: &str = "https://clientsettingscdn.roblox.com/v2/client-version/WindowsPlayer";
pub const DEPLOY_URL: &str = "https://setup.rbxcdn.com";
pub const PLAYER_EXE: &str = "RobloxPlayerBeta.exe";
const AUTH_URL: &str = "https://auth.roblox.com/v2/login";

const PACKAGE_DIRS: [(&str, &str); 21] = [
    ("RobloxApp.zip", ""),
    ("shaders.zip", "shaders"),
    ("ssl.zip", "ssl"),
    ("WebView2.zip", ""),
    ("WebView2RuntimeInstaller.zip", "WebView2RuntimeInstaller"),
    ("content-avatar.zip", "content/avatar"),
    ("content-configs.zip", "content/configs"),
    ("content-fonts.zip", "content/fonts"),
    ("content-sky.zip", "content/sky"),
    ("content-sounds.zip", "content/sounds"),
    ("content-textures2.zip", "content/textures"),
    ("content-models.zip", "content/models"),
    ("content-textures3.zip", "PlatformContent/pc/textures"),
    ("content-terrain.zip", "PlatformContent/pc/terrain"),
    ("content-platform-fonts.zip", "PlatformContent/pc/fonts"),
    ("content-platform-dictionaries.zip", "PlatformContent/pc/shared_compression_dictionaries"),
    ("extracontent-luapackages.zip", "ExtraContent/LuaPackages"),
    ("extracontent-translations.zip", "ExtraContent/translations"),
    ("extracontent-models.zip", "ExtraContent/models"),
    ("extracontent-textures.zip", "ExtraContent/textures"),
    ("extracontent-places.zip", "ExtraContent/places"),
];

/// Progress and lifecycle events sent to the UI.
#[derive(Debug, Clone)]
pub enum Event {
    Status(String),
    Progress { done: u64, total: u64 },
    Installed { guid: String, version: String },
    Launched,
    Finished,
    Failed(String),
    Cancelled,
}

pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;

#[derive(Debug)]
pub struct Cancelled;
impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "cancelled") }
}
impl std::error::Error for Cancelled {}

#[derive(Deserialize, Clone, Debug)]
pub struct ClientVersion {
    pub version: String,
    #[serde(rename = "clientVersionUpload")]
    pub upload: String,
}

#[allow(dead_code)]
pub struct Installed {
    pub dir: PathBuf,
    pub guid: String,
    pub version: String,
}

pub fn http() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(branding::USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(1800))
        .build()?)
}

pub fn latest_version(client: &reqwest::blocking::Client) -> Result<ClientVersion> {
    let mut last: Option<anyhow::Error> = None;
    for attempt in 1..=3 {
        let res = client
            .get(VERSION_URL)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(anyhow::Error::from)
            .and_then(|r| r.json::<ClientVersion>().map_err(anyhow::Error::from));
        match res {
            Ok(v) => return Ok(v),
            Err(e) => {
                warn!("version lookup attempt {attempt} failed: {e:#}");
                last = Some(e);
                std::thread::sleep(Duration::from_millis(600 * attempt));
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("version lookup failed")))
}

struct Package {
    name: String,
    md5: String,
    size: u64,
}

fn parse_manifest(text: &str) -> Result<Vec<Package>> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    match lines.next() {
        Some("v0") => {}
        other => bail!("unexpected manifest header: {:?}", other),
    }
    let rest: Vec<&str> = lines.collect();
    if rest.len() % 4 != 0 {
        bail!("manifest has {} lines, expected a multiple of 4", rest.len());
    }
    Ok(rest
        .chunks(4)
        .map(|c| Package {
            name: c[0].to_string(),
            md5: c[1].to_lowercase(),
            size: c[2].parse().unwrap_or(0),
        })
        .collect())
}

struct InstallLock {
    path: PathBuf,
}

impl InstallLock {
    fn acquire(p: &Paths) -> Result<Self> {
        let path = p.lock();
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            // A lock that has not been touched for 3 minutes belongs to a crashed instance.
            if let Ok(meta) = fs::metadata(&path) {
                let stale = meta
                    .modified()
                    .ok()
                    .and_then(|m| m.elapsed().ok())
                    .map(|age| age > Duration::from_secs(180))
                    .unwrap_or(true);
                if stale {
                    let _ = fs::remove_file(&path);
                }
            }
            match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Self { path }),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    if Instant::now() > deadline {
                        bail!("another RustBlox window is still installing; try again in a minute");
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    fn touch(&self) {
        let _ = fs::write(&self.path, b"x");
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(Cancelled.into());
    }
    Ok(())
}

fn download_package(
    client: &reqwest::blocking::Client,
    url: &str,
    expect_md5: &str,
    base: u64,
    total: u64,
    emit: &Emit,
    lock: &InstallLock,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    let mut last: Option<anyhow::Error> = None;
    for attempt in 1..=3 {
        check_cancel(cancel)?;
        let res: Result<Vec<u8>> = (|| {
            let mut resp = client.get(url).send()?.error_for_status()?;
            let mut buf: Vec<u8> = Vec::new();
            let mut hasher = Md5::new();
            let mut chunk = vec![0u8; 64 * 1024];
            let mut last_emit = Instant::now();
            let mut last_touch = Instant::now();
            loop {
                check_cancel(cancel)?;
                let n = resp.read(&mut chunk)?;
                if n == 0 {
                    break;
                }
                hasher.update(&chunk[..n]);
                buf.extend_from_slice(&chunk[..n]);
                if last_emit.elapsed() > Duration::from_millis(100) {
                    emit(Event::Progress { done: base + buf.len() as u64, total });
                    last_emit = Instant::now();
                }
                if last_touch.elapsed() > Duration::from_secs(5) {
                    lock.touch();
                    last_touch = Instant::now();
                }
            }
            let digest = format!("{:x}", hasher.finalize());
            if digest != expect_md5 {
                bail!("checksum mismatch (got {digest}, expected {expect_md5})");
            }
            Ok(buf)
        })();
        match res {
            Ok(b) => return Ok(b),
            Err(e) if e.downcast_ref::<Cancelled>().is_some() => return Err(e),
            Err(e) => {
                warn!("download attempt {attempt} for {url} failed: {e:#}");
                last = Some(e);
                std::thread::sleep(Duration::from_secs(attempt));
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("download failed")))
}

pub fn extract_zip(data: &[u8], dest: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(Cursor::new(data))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let rel = match entry.enclosed_name() {
            Some(p) => p.to_owned(),
            None => {
                warn!("skipping unsafe zip entry {}", entry.name());
                continue;
            }
        };
        let out = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut f = fs::File::create(&out)?;
            io::copy(&mut entry, &mut f)?;
        }
    }
    Ok(())
}

fn existing(p: &Paths, guid: &str) -> Option<PathBuf> {
    let dir = p.versions().join(guid);
    if dir.join(PLAYER_EXE).exists() { Some(dir) } else { None }
}

/// Makes sure the newest Roblox client is installed. Falls back to the installed
/// client when Roblox's servers cannot be reached.
pub fn ensure_installed(
    p: &Paths,
    cur_guid: Option<&str>,
    cur_label: Option<&str>,
    force: bool,
    emit: &Emit,
    cancel: &AtomicBool,
) -> Result<Installed> {
    emit(Event::Status("Checking for Roblox updates...".into()));
    let client = http()?;
    let latest = match latest_version(&client) {
        Ok(v) => v,
        Err(e) => {
            if let Some(g) = cur_guid {
                if let Some(dir) = existing(p, g) {
                    warn!("offline, using installed client: {e:#}");
                    emit(Event::Status("Could not reach Roblox, using the installed version.".into()));
                    return Ok(Installed {
                        dir,
                        guid: g.to_string(),
                        version: cur_label.unwrap_or("installed").to_string(),
                    });
                }
            }
            return Err(e.context("could not reach Roblox to download the client"));
        }
    };
    let guid = latest.upload.clone();
    info!("latest client {} ({})", latest.version, guid);

    if !force {
        if let Some(dir) = existing(p, &guid) {
            emit(Event::Installed { guid: guid.clone(), version: latest.version.clone() });
            return Ok(Installed { dir, guid, version: latest.version });
        }
    }

    let lock = InstallLock::acquire(p)?;
    if !force {
        if let Some(dir) = existing(p, &guid) {
            emit(Event::Installed { guid: guid.clone(), version: latest.version.clone() });
            return Ok(Installed { dir, guid, version: latest.version });
        }
    }

    let final_dir = p.versions().join(&guid);
    let partial = p.versions().join(format!("{guid}.partial"));
    if partial.exists() {
        fs::remove_dir_all(&partial).context("clearing a previous partial install")?;
    }
    fs::create_dir_all(&partial)?;

    emit(Event::Status(format!("Downloading Roblox {}...", latest.version)));
    let manifest_text = client
        .get(format!("{DEPLOY_URL}/{guid}-rbxPkgManifest.txt"))
        .send()?
        .error_for_status()?
        .text()?;
    let packages: Vec<Package> = parse_manifest(&manifest_text)?
        .into_iter()
        .filter(|pk| PACKAGE_DIRS.iter().any(|(n, _)| *n == pk.name))
        .collect();
    if packages.is_empty() {
        bail!("Roblox returned an empty package list");
    }
    let total: u64 = packages.iter().map(|pk| pk.size).sum::<u64>().max(1);
    let mut base: u64 = 0;

    for (i, pkg) in packages.iter().enumerate() {
        check_cancel(cancel)?;
        let sub = PACKAGE_DIRS.iter().find(|(n, _)| *n == pkg.name).map(|(_, d)| *d).unwrap_or("");
        emit(Event::Status(format!("Downloading {} ({}/{})", pkg.name, i + 1, packages.len())));
        let bytes = download_package(
            &client,
            &format!("{DEPLOY_URL}/{guid}-{}", pkg.name),
            &pkg.md5,
            base,
            total,
            emit,
            &lock,
            cancel,
        )
        .with_context(|| format!("downloading {}", pkg.name))?;
        base += pkg.size;
        emit(Event::Status(format!("Extracting {}", pkg.name)));
        let target = if sub.is_empty() { partial.clone() } else { partial.join(sub) };
        fs::create_dir_all(&target)?;
        extract_zip(&bytes, &target).with_context(|| format!("extracting {}", pkg.name))?;
        emit(Event::Progress { done: base, total });
        lock.touch();
    }

    if !partial.join(PLAYER_EXE).exists() {
        bail!("{PLAYER_EXE} is missing after extraction; the install was discarded");
    }
    fs::write(
        partial.join("AppSettings.xml"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<Settings>\r\n\t<ContentFolder>content</ContentFolder>\r\n\t<BaseUrl>http://www.roblox.com</BaseUrl>\r\n</Settings>\r\n",
    )?;

    if final_dir.exists() {
        fs::remove_dir_all(&final_dir).context("removing the previous copy of this version")?;
    }
    fs::rename(&partial, &final_dir)?;

    if let Ok(rd) = fs::read_dir(p.versions()) {
        for entry in rd.flatten() {
            if entry.path() != final_dir && entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                debug!("removing old version {}", entry.path().display());
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }

    emit(Event::Installed { guid: guid.clone(), version: latest.version.clone() });
    info!("install complete");
    Ok(Installed { dir: final_dir, guid, version: latest.version })
}

pub fn apply_flags(dir: &Path, flags: &BTreeMap<String, serde_json::Value>) -> Result<()> {
    let settings_dir = dir.join("ClientSettings");
    let file = settings_dir.join("ClientAppSettings.json");
    if flags.is_empty() {
        if file.exists() {
            fs::remove_file(&file)?;
        }
        return Ok(());
    }
    fs::create_dir_all(&settings_dir)?;
    fs::write(&file, serde_json::to_string_pretty(flags)?)?;
    info!("applied {} FastFlag(s)", flags.len());
    Ok(())
}

/// Builds the command-line for RobloxPlayerBeta from a protocol URI (or the app home when None).
pub fn build_args(uri: Option<&str>) -> Result<Vec<String>> {
    let uri = match uri {
        Some(u) => u.trim().trim_matches('"'),
        None => return Ok(vec!["--app".into()]),
    };
    if uri.starts_with("roblox:") {
        // roblox://experiences/start?placeId=... is understood by the player as-is.
        return Ok(vec![uri.to_string()]);
    }
    let body = uri
        .strip_prefix("roblox-player:")
        .ok_or_else(|| anyhow!("not a roblox-player: or roblox: link"))?;
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    for part in body.split('+') {
        if let Some((k, v)) = part.split_once(':') {
            map.insert(k.to_lowercase(), v.to_string());
        }
    }
    let ticket = match map.get("gameinfo") {
        Some(t) if !t.is_empty() => t,
        _ => return Ok(vec!["--app".into()]),
    };
    let join_url = urlencoding::decode(
        map.get("placelauncherurl").ok_or_else(|| anyhow!("link is missing placelauncherurl"))?,
    )
    .map_err(|e| anyhow!("bad placelauncherurl: {e}"))?
    .into_owned();
    let tracker = map.get("browsertrackerid").ok_or_else(|| anyhow!("link is missing browsertrackerid"))?;
    let launchtime = map.get("launchtime").ok_or_else(|| anyhow!("link is missing launchtime"))?;
    let rloc = map.get("robloxlocale").cloned().unwrap_or_else(|| "en_us".into());
    let gloc = map.get("gamelocale").cloned().unwrap_or_else(|| "en_us".into());
    Ok(vec![
        "--play".into(),
        "-a".into(),
        AUTH_URL.into(),
        "-t".into(),
        ticket.clone(),
        "-j".into(),
        join_url,
        "-b".into(),
        tracker.clone(),
        format!("--launchtime={launchtime}"),
        "--rloc".into(),
        rloc,
        "--gloc".into(),
        gloc,
    ])
}

pub fn spawn_player(dir: &Path, args: &[String]) -> Result<std::process::Child> {
    let exe = dir.join(PLAYER_EXE);
    std::process::Command::new(&exe)
        .args(args)
        .current_dir(dir)
        .spawn()
        .with_context(|| format!("starting {}", exe.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_parses_groups_of_four() {
        let text = "v0\nRobloxApp.zip\nABCDEF0123\n100\n200\nshaders.zip\n0011\n10\n20\n";
        let pk = parse_manifest(text).expect("parse");
        assert_eq!(pk.len(), 2);
        assert_eq!(pk[0].name, "RobloxApp.zip");
        assert_eq!(pk[0].md5, "abcdef0123");
        assert_eq!(pk[0].size, 100);
        assert!(parse_manifest("v1\nx").is_err());
        assert!(parse_manifest("v0\na\nb\nc").is_err());
    }

    #[test]
    fn web_link_becomes_play_arguments() {
        let uri = "roblox-player:1+launchmode:play+gameinfo:TICKET123+launchtime:1700000000000+placelauncherurl:https%3A%2F%2Fassetgame.roblox.com%2Fgame%2FPlaceLauncher.ashx%3Frequest%3DRequestGame%26placeId%3D42+browsertrackerid:555+robloxLocale:en_us+gameLocale:de_de+channel:+LaunchExp:InApp";
        let a = build_args(Some(uri)).expect("args");
        assert_eq!(a[0], "--play");
        assert!(a.windows(2).any(|w| w[0] == "-t" && w[1] == "TICKET123"));
        assert!(a.windows(2).any(|w| w[0] == "-j" && w[1] == "https://assetgame.roblox.com/game/PlaceLauncher.ashx?request=RequestGame&placeId=42"));
        assert!(a.windows(2).any(|w| w[0] == "-b" && w[1] == "555"));
        assert!(a.contains(&"--launchtime=1700000000000".to_string()));
        assert!(a.windows(2).any(|w| w[0] == "--gloc" && w[1] == "de_de"));
    }

    #[test]
    fn no_uri_or_no_ticket_opens_the_app() {
        assert_eq!(build_args(None).unwrap(), vec!["--app"]);
        assert_eq!(build_args(Some("roblox-player:1+launchmode:app")).unwrap(), vec!["--app"]);
    }

    #[test]
    fn roblox_scheme_is_passed_through_and_junk_is_rejected() {
        let u = "roblox://experiences/start?placeId=1&gameInstanceId=abc";
        assert_eq!(build_args(Some(u)).unwrap(), vec![u.to_string()]);
        assert!(build_args(Some("https://example.com")).is_err());
        assert!(build_args(Some("roblox-player:1+gameinfo:T+launchtime:1")).is_err());
    }
}
