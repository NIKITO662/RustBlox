//! The background job behind "Launch", "Update" and "Repair".

use crate::activity;
use crate::config::Settings;
use crate::mods;
use crate::paths::Paths;
use crate::roblox::{self, Cancelled, Emit, Event};
use anyhow::Result;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::SystemTime;
use tracing::{error, warn};

#[derive(Clone, Debug)]
pub enum Action {
    Launch(Option<String>),
    Install { force: bool },
}

pub fn spawn(paths: Paths, settings: Settings, action: Action, emit: Emit, cancel: Arc<AtomicBool>) -> JoinHandle<()> {
    std::thread::spawn(move || match flow(&paths, &settings, &action, &emit, &cancel) {
        Ok(()) => {}
        Err(e) if e.downcast_ref::<Cancelled>().is_some() => emit(Event::Cancelled),
        Err(e) => {
            error!("launch flow failed: {e:#}");
            emit(Event::Failed(format!("{e:#}")));
        }
    })
}

fn flow(p: &Paths, s: &Settings, action: &Action, emit: &Emit, cancel: &Arc<AtomicBool>) -> Result<()> {
    let (force, uri) = match action {
        Action::Launch(u) => (false, u.clone()),
        Action::Install { force } => (*force, None),
    };

    // Validate the link before touching the network so bad links fail fast.
    let args = match action {
        Action::Launch(_) => Some(roblox::build_args(uri.as_deref())?),
        Action::Install { .. } => None,
    };

    let inst = roblox::ensure_installed(p, s.installed_version.as_deref(), s.installed_label.as_deref(), force, emit, cancel)?;

    let args = match args {
        Some(a) => a,
        None => {
            emit(Event::Finished);
            return Ok(());
        }
    };

    emit(Event::Status("Applying mods...".into()));
    if let Err(e) = mods::restore(p) {
        warn!("restoring mods failed: {e:#}");
    }
    match mods::apply(p, &inst.dir, &s.disabled_mods) {
        Ok(n) if n > 0 => emit(Event::Status(format!("Applied {n} mod file(s)"))),
        Ok(_) => {}
        Err(e) => {
            warn!("applying mods failed: {e:#}");
            emit(Event::Status("Some mods could not be applied; launching anyway.".into()));
        }
    }

    if let Err(e) = roblox::apply_flags(&inst.dir, &s.fflags) {
        warn!("applying FastFlags failed: {e:#}");
    }

    emit(Event::Status("Starting Roblox...".into()));
    let started = SystemTime::now();
    let child = roblox::spawn_player(&inst.dir, &args)?;
    drop(child); // Windows leaves no zombie; activity watches the process by name instead.
    emit(Event::Launched);

    let wants_session = s.activity_tracking || (s.discord_rpc && !crate::branding::DISCORD_APP_ID.is_empty());
    if wants_session {
        activity::run_session(p, s, started, cancel_never());
    }
    Ok(())
}

fn cancel_never() -> &'static AtomicBool {
    static NEVER: AtomicBool = AtomicBool::new(false);
    &NEVER
}
