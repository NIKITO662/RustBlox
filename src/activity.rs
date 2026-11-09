//! Activity tracking: follows the Roblox player log to learn which server you are in,
//! keeps a play history, and publishes a live snapshot for the UI and Discord.

use crate::branding;
use crate::config::Settings;
use crate::paths::{roblox_logs_dir, Paths};
use crate::rpc::Rpc;
use crate::tools;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tracing::{debug, info, warn};

#[derive(Clone, Serialize, Deserialize, Default, PartialEq, Debug)]
pub struct Session {
    pub place_id: u64,
    pub job_id: String,
    pub server_ip: String,
    pub joined: bool,
    pub started_unix: i64,
    pub game_name: Option<String>,
    pub creator: Option<String>,
    pub location: Option<String>,
}

impl Session {
    pub fn web_link(&self) -> String {
        format!("https://www.roblox.com/games/start?placeId={}&gameInstanceId={}", self.place_id, self.job_id)
    }
    pub fn deep_link(&self) -> String {
        format!("roblox://experiences/start?placeId={}&gameInstanceId={}", self.place_id, self.job_id)
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct HistoryEntry {
    pub place_id: u64,
    pub job_id: String,
    pub game_name: String,
    pub location: String,
    pub started_unix: i64,
    pub duration_secs: i64,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Live {
    pub session: Option<Session>,
    pub updated_unix: i64,
}

pub fn now_unix() -> i64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn load_history(p: &Paths) -> Vec<HistoryEntry> {
    fs::read_to_string(p.history()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_history(p: &Paths, h: &[HistoryEntry]) {
    let keep = &h[h.len().saturating_sub(100)..];
    if let Ok(text) = serde_json::to_string_pretty(keep) {
        let _ = fs::write(p.history(), text);
    }
}

pub fn read_live(p: &Paths) -> Option<Session> {
    let live: Live = fs::read_to_string(p.live()).ok().and_then(|t| serde_json::from_str(&t).ok())?;
    // A snapshot that has not been refreshed for a while belongs to a dead process.
    if now_unix() - live.updated_unix > 15 {
        return None;
    }
    live.session
}

struct Patterns {
    join: Regex,
    udmux: Regex,
    server: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| Patterns {
        join: Regex::new(r"! Joining game '([0-9a-fA-F\-]{36})' place (\d+) at ([0-9.]+)").expect("join regex"),
        udmux: Regex::new(r"UDMUX Address = ([0-9.]+), Port = \d+").expect("udmux regex"),
        server: Regex::new(r"serverId: ([0-9.]+)\|\d+").expect("server regex"),
    })
}

#[derive(Debug, PartialEq)]
pub enum Update {
    Joining,
    Joined,
    Left(HistoryEntry),
    None,
}

#[derive(Default)]
pub struct Tracker {
    pub current: Option<Session>,
}

impl Tracker {
    fn finish(&mut self) -> Option<HistoryEntry> {
        let s = self.current.take()?;
        if !s.joined {
            return None;
        }
        Some(HistoryEntry {
            place_id: s.place_id,
            job_id: s.job_id,
            game_name: s.game_name.unwrap_or_else(|| "Unknown experience".into()),
            location: s.location.unwrap_or_else(|| "Unknown".into()),
            started_unix: s.started_unix,
            duration_secs: (now_unix() - s.started_unix).max(0),
        })
    }

    /// Feeds one log line; returns what changed.
    pub fn feed(&mut self, line: &str) -> Update {
        let pat = patterns();
        if line.contains("! Joining game") {
            if let Some(c) = pat.join.captures(line) {
                let previous = self.finish();
                self.current = Some(Session {
                    place_id: c[2].parse().unwrap_or(0),
                    job_id: c[1].to_string(),
                    server_ip: c[3].to_string(),
                    joined: false,
                    started_unix: now_unix(),
                    ..Default::default()
                });
                return match previous {
                    Some(h) => Update::Left(h),
                    None => Update::Joining,
                };
            }
        } else if line.contains("UDMUX Address") {
            if let (Some(c), Some(s)) = (pat.udmux.captures(line), self.current.as_mut()) {
                if !s.joined {
                    s.server_ip = c[1].to_string();
                }
            }
        } else if line.contains("serverId:") {
            if pat.server.is_match(line) {
                if let Some(s) = self.current.as_mut() {
                    if !s.joined {
                        s.joined = true;
                        s.started_unix = now_unix();
                        return Update::Joined;
                    }
                }
            }
        } else if line.contains("Time to disconnect replication data") {
            return match self.finish() {
                Some(h) => Update::Left(h),
                None => Update::None,
            };
        }
        Update::None
    }
}

#[derive(Clone, Debug)]
struct Info {
    game_name: Option<String>,
    creator: Option<String>,
    location: Option<String>,
}

fn lookup_info(job_id: String, place_id: u64, ip: String, want_location: bool, out: Arc<Mutex<Vec<(String, Info)>>>) {
    std::thread::spawn(move || {
        let client = match reqwest::blocking::Client::builder()
            .user_agent(branding::USER_AGENT)
            .timeout(Duration::from_secs(10))
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };
        let mut info = Info { game_name: None, creator: None, location: None };

        let universe: Option<u64> = client
            .get(format!("https://apis.roblox.com/universes/v1/places/{place_id}/universe"))
            .send()
            .ok()
            .and_then(|r| r.json::<serde_json::Value>().ok())
            .and_then(|v| v.get("universeId").and_then(|u| u.as_u64()));
        if let Some(u) = universe {
            if let Some(v) = client
                .get(format!("https://games.roblox.com/v1/games?universeIds={u}"))
                .send()
                .ok()
                .and_then(|r| r.json::<serde_json::Value>().ok())
            {
                if let Some(g) = v.get("data").and_then(|d| d.get(0)) {
                    info.game_name = g.get("name").and_then(|n| n.as_str()).map(String::from);
                    info.creator = g.get("creator").and_then(|c| c.get("name")).and_then(|n| n.as_str()).map(String::from);
                }
            }
        }
        if want_location && !ip.is_empty() {
            if let Some(v) = client
                .get(format!("https://ipinfo.io/{ip}/json"))
                .send()
                .ok()
                .and_then(|r| r.json::<serde_json::Value>().ok())
            {
                let part = |k: &str| v.get(k).and_then(|x| x.as_str()).filter(|s| !s.is_empty()).map(String::from);
                let pieces: Vec<String> = [part("city"), part("region"), part("country")].into_iter().flatten().collect();
                if !pieces.is_empty() {
                    info.location = Some(pieces.join(", "));
                }
            }
        }
        if let Ok(mut guard) = out.lock() {
            guard.push((job_id, info));
        }
    });
}

struct LogReader {
    path: Option<PathBuf>,
    offset: u64,
    partial: String,
}

impl LogReader {
    fn new() -> Self {
        Self { path: None, offset: 0, partial: String::new() }
    }

    fn pick(started: SystemTime) -> Option<PathBuf> {
        let dir = roblox_logs_dir()?;
        let threshold = started.checked_sub(Duration::from_secs(10)).unwrap_or(started);
        let mut best: Option<(SystemTime, PathBuf)> = None;
        for e in fs::read_dir(dir).ok()?.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.ends_with(".log") || !name.contains("_Player_") || name.contains("_last") {
                continue;
            }
            if let Ok(m) = e.metadata().and_then(|m| m.modified()) {
                if m >= threshold && best.as_ref().map(|(t, _)| m > *t).unwrap_or(true) {
                    best = Some((m, e.path()));
                }
            }
        }
        best.map(|(_, p)| p)
    }

    fn poll(&mut self, started: SystemTime) -> Vec<String> {
        let newest = match Self::pick(started) {
            Some(p) => p,
            None => return Vec::new(),
        };
        if self.path.as_ref() != Some(&newest) {
            self.path = Some(newest.clone());
            self.offset = 0;
            self.partial.clear();
        }
        let mut file = match fs::File::open(&newest) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut bytes = Vec::new();
        if file.read_to_end(&mut bytes).is_err() {
            return Vec::new();
        }
        self.offset += bytes.len() as u64;
        self.partial.push_str(&String::from_utf8_lossy(&bytes));
        let mut lines = Vec::new();
        while let Some(i) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=i).collect();
            lines.push(line.trim_end().to_string());
        }
        lines
    }
}

fn write_live(p: &Paths, session: &Option<Session>) {
    let live = Live { session: session.clone(), updated_unix: now_unix() };
    if let Ok(text) = serde_json::to_string(&live) {
        let _ = fs::write(p.live(), text);
    }
}

/// Runs until Roblox exits (or `cancel` is set). Call from a background thread.
pub fn run_session(p: &Paths, s: &Settings, started: SystemTime, cancel: &AtomicBool) {
    info!("activity session started");
    let mut history = load_history(p);
    let mut tracker = Tracker::default();
    let mut reader = LogReader::new();
    let mut rpc = if s.discord_rpc && !branding::DISCORD_APP_ID.is_empty() { Some(Rpc::new(branding::DISCORD_APP_ID)) } else { None };
    let infos: Arc<Mutex<Vec<(String, Info)>>> = Arc::new(Mutex::new(Vec::new()));

    let begin = Instant::now();
    let mut seen_running = false;
    let mut missing_polls = 0u32;
    let mut last_written: Option<Session> = None;
    let mut last_heartbeat = Instant::now() - Duration::from_secs(60);

    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let mut changed = false;

        if s.activity_tracking || rpc.is_some() {
            for line in reader.poll(started) {
                match tracker.feed(&line) {
                    Update::Joining => changed = true,
                    Update::Joined => {
                        changed = true;
                        if let Some(cur) = &tracker.current {
                            debug!("joined place {} job {}", cur.place_id, cur.job_id);
                            lookup_info(cur.job_id.clone(), cur.place_id, cur.server_ip.clone(), s.server_location, infos.clone());
                        }
                    }
                    Update::Left(h) => {
                        changed = true;
                        history.push(h);
                        save_history(p, &history);
                    }
                    Update::None => {}
                }
            }
        }

        let ready: Vec<(String, Info)> = infos.lock().map(|mut g| g.drain(..).collect()).unwrap_or_default();
        for (job, info) in ready {
            if let Some(cur) = tracker.current.as_mut() {
                if cur.job_id == job {
                    cur.game_name = info.game_name.or(cur.game_name.take());
                    cur.creator = info.creator.or(cur.creator.take());
                    cur.location = info.location.or(cur.location.take());
                    changed = true;
                }
            }
        }

        let snapshot = tracker.current.clone();
        if changed || last_heartbeat.elapsed() > Duration::from_secs(5) {
            if s.activity_tracking && (changed || snapshot != last_written || last_heartbeat.elapsed() > Duration::from_secs(5)) {
                write_live(p, &snapshot);
                last_written = snapshot.clone();
            }
            last_heartbeat = Instant::now();
        }
        if let Some(r) = rpc.as_mut() {
            r.update(&snapshot, s);
        }

        if tools::roblox_running() {
            seen_running = true;
            missing_polls = 0;
        } else {
            missing_polls += 1;
            let grace = if seen_running { 4 } else { 90 };
            if missing_polls >= grace {
                break;
            }
        }
        if !seen_running && begin.elapsed() > Duration::from_secs(120) {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }

    if let Some(h) = tracker.finish() {
        history.push(h);
        save_history(p, &history);
    }
    let _ = fs::remove_file(p.live());
    if let Some(mut r) = rpc {
        r.close();
    }
    warn!("activity session ended");
}

#[cfg(test)]
mod tests {
    use super::*;

    const JOIN: &str = "2026-11-08T10:00:00.000Z,1.0,abcd,6 [FLog::Output] ! Joining game '0f9ab8a2-3b4c-4d5e-8f60-123456789abc' place 920587237 at 128.116.1.5";
    const UDMUX: &str = "2026-11-08T10:00:01.000Z,1.1,abcd,6 [FLog::Network] UDMUX Address = 128.116.9.9, Port = 53217 | RCC Server Address = 128.116.1.5, Port = 53217";
    const SERVER: &str = "2026-11-08T10:00:03.000Z,3.0,abcd,6 [FLog::Network] serverId: 128.116.9.9|53217";
    const LEAVE: &str = "2026-11-08T10:30:03.000Z,1803.0,abcd,6 [FLog::Network] Time to disconnect replication data: 0.000";

    #[test]
    fn full_join_and_leave_cycle() {
        let mut t = Tracker::default();
        assert_eq!(t.feed("unrelated line"), Update::None);
        assert_eq!(t.feed(JOIN), Update::Joining);
        let s = t.current.clone().expect("session");
        assert_eq!(s.place_id, 920587237);
        assert_eq!(s.job_id, "0f9ab8a2-3b4c-4d5e-8f60-123456789abc");
        assert_eq!(s.server_ip, "128.116.1.5");
        assert!(!s.joined);
        t.feed(UDMUX);
        assert_eq!(t.current.as_ref().unwrap().server_ip, "128.116.9.9");
        assert_eq!(t.feed(SERVER), Update::Joined);
        assert!(t.current.as_ref().unwrap().joined);
        match t.feed(LEAVE) {
            Update::Left(h) => {
                assert_eq!(h.place_id, 920587237);
                assert_eq!(h.game_name, "Unknown experience");
            }
            other => panic!("expected Left, got {other:?}"),
        }
        assert!(t.current.is_none());
    }

    #[test]
    fn failed_join_is_not_recorded() {
        let mut t = Tracker::default();
        t.feed(JOIN);
        assert_eq!(t.feed(LEAVE), Update::None);
        assert!(t.current.is_none());
    }

    #[test]
    fn teleport_records_previous_server() {
        let mut t = Tracker::default();
        t.feed(JOIN);
        t.feed(SERVER);
        let second = JOIN.replace("920587237", "111").replace("0f9ab8a2", "1a9ab8a2");
        assert!(matches!(t.feed(&second), Update::Left(_)));
        assert_eq!(t.current.as_ref().unwrap().place_id, 111);
    }
}
