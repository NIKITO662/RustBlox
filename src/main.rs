#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod activity;
mod branding;
mod config;
mod launcher;
mod mods;
mod news;
mod paths;
mod roblox;
mod rpc;
mod tools;
mod ui;

use std::fs::OpenOptions;
use std::sync::Mutex;

fn init_logging(p: &paths::Paths) {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let file = OpenOptions::new().create(true).append(true).open(p.logs().join(format!("rustblox-{stamp}.log")));
    if let Ok(file) = file {
        let _ = tracing_subscriber::fmt()
            .with_writer(Mutex::new(file))
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
    }
    // Keep only the 10 newest launcher logs.
    if let Ok(rd) = std::fs::read_dir(p.logs()) {
        let mut files: Vec<_> = rd.flatten().collect();
        files.sort_by_key(|e| e.file_name());
        let excess = files.len().saturating_sub(10);
        for e in files.into_iter().take(excess) {
            let _ = std::fs::remove_file(e.path());
        }
    }
    std::panic::set_hook(Box::new(|info| {
        tracing::error!("panic: {info}");
    }));
}

fn main() {
    let paths = match paths::Paths::new() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("cannot start: {e:#}");
            std::process::exit(1);
        }
    };
    init_logging(&paths);
    tracing::info!("{} {} starting", branding::APP_NAME, branding::APP_VERSION);

    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = args.first().map(|s| s.as_str()).unwrap_or("");

    let code = match first {
        "--register" => report(tools::register(&paths)),
        "--unregister" => report(tools::unregister(&paths)),
        "" => run_ui(paths, ui::Mode::Main),
        "--launch" => run_ui(paths, ui::Mode::Launch(None)),
        uri if uri.starts_with("roblox-player:") || uri.starts_with("roblox:") => {
            run_ui(paths, ui::Mode::Launch(Some(uri.to_string())))
        }
        other => {
            eprintln!("unknown argument: {other}");
            2
        }
    };
    std::process::exit(code);
}

fn report(r: anyhow::Result<()>) -> i32 {
    match r {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!("{e:#}");
            eprintln!("error: {e:#}");
            1
        }
    }
}

fn run_ui(paths: paths::Paths, mode: ui::Mode) -> i32 {
    let result = ui::run(paths, mode);
    // Keep the process alive while the background job (activity / Discord) is still running.
    ui::wait_for_background();
    match result {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!("UI error: {e}");
            eprintln!("error: {e}");
            1
        }
    }
}
