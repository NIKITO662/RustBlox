//! The launcher window (egui). Two modes: the full app, and a compact progress window
//! used when Roblox is started from a web link.

use crate::activity::{self, HistoryEntry, Session};
use crate::branding;
use crate::config::{self, Settings, FPS_FLAG};
use crate::launcher::{self, Action};
use crate::mods::{self, Pack};
use crate::news::{self, Feed};
use crate::paths::{roblox_logs_dir, Paths};
use crate::roblox::{self, Event};
use crate::tools;
use eframe::egui::{self, Align, Color32, Layout, RichText, Stroke, Vec2};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const ACCENT: Color32 = Color32::from_rgb(255, 107, 61);
const CARD: Color32 = Color32::from_rgb(24, 27, 48);
const CARD_LINE: Color32 = Color32::from_rgb(42, 46, 78);
const LOGO_PNG: &[u8] = include_bytes!("../assets/logo.png");

static BACKGROUND: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

pub enum Mode {
    Main,
    Launch(Option<String>),
}

pub fn wait_for_background() {
    let handle = BACKGROUND.lock().ok().and_then(|mut g| g.take());
    if let Some(h) = handle {
        let _ = h.join();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Home,
    News,
    Mods,
    Settings,
    Activity,
    Tools,
    About,
}

enum UiMsg {
    Launch(Event),
    News(Result<Feed, String>),
    Latest(Result<roblox::ClientVersion, String>),
}

struct Job {
    label: &'static str,
    status: String,
    done: u64,
    total: u64,
    started: Instant,
    error: Option<String>,
    finished: bool,
    launched: bool,
    cancelled: bool,
    close_at: Option<Instant>,
    cancel: Arc<AtomicBool>,
}

impl Job {
    fn active(&self) -> bool {
        !self.finished && !self.launched && !self.cancelled && self.error.is_none()
    }
}

struct App {
    launch_window: bool,
    paths: Paths,
    settings: Settings,
    tab: Tab,
    last_tab: Tab,
    tx: mpsc::Sender<UiMsg>,
    rx: mpsc::Receiver<UiMsg>,
    logo: egui::TextureHandle,
    trivia_seed: usize,

    job: Option<Job>,
    job_visible: bool,

    feed: Feed,
    news_status: String,
    news_highlight: BTreeSet<String>,
    suppress_modal: bool,

    latest: Option<roblox::ClientVersion>,
    latest_status: String,

    packs: Vec<Pack>,
    new_pack: String,
    confirm_delete: Option<String>,

    new_flag_name: String,
    new_flag_value: String,

    live: Option<Session>,
    history: Vec<HistoryEntry>,
    last_live: Instant,

    toast: Option<(String, bool, Instant)>,
    confirm_uninstall: bool,
}

pub fn run(paths: Paths, mode: Mode) -> eframe::Result<()> {
    let logo = image::load_from_memory(LOGO_PNG).expect("bundled logo is valid").to_rgba8();
    let (w, h) = logo.dimensions();
    let icon = egui::IconData { rgba: logo.clone().into_raw(), width: w, height: h };

    let is_launch = matches!(mode, Mode::Launch(_));
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(branding::APP_NAME)
        .with_icon(Arc::new(icon));
    viewport = if is_launch {
        viewport.with_inner_size([520.0, 400.0]).with_resizable(false).with_maximize_button(false)
    } else {
        viewport.with_inner_size([980.0, 660.0]).with_min_inner_size([840.0, 560.0])
    };
    let options = eframe::NativeOptions { viewport, centered: true, ..Default::default() };

    eframe::run_native(
        branding::APP_NAME,
        options,
        Box::new(move |cc| {
            setup_style(&cc.egui_ctx);
            let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], logo.as_raw());
            let tex = cc.egui_ctx.load_texture("logo", color, egui::TextureOptions::LINEAR);
            Ok(Box::new(App::new(paths, mode, tex)))
        }),
    )
}

fn setup_style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::from_rgb(15, 17, 31);
    v.window_fill = Color32::from_rgb(21, 24, 43);
    v.extreme_bg_color = Color32::from_rgb(10, 11, 22);
    v.faint_bg_color = Color32::from_rgb(22, 25, 44);
    v.selection.bg_fill = Color32::from_rgb(190, 78, 50);
    v.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    v.hyperlink_color = Color32::from_rgb(255, 150, 110);
    let r = egui::Rounding::same(8.0);
    v.widgets.noninteractive.rounding = r;
    v.widgets.inactive.rounding = r;
    v.widgets.hovered.rounding = r;
    v.widgets.active.rounding = r;
    v.widgets.inactive.bg_fill = Color32::from_rgb(38, 42, 72);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(38, 42, 72);
    v.widgets.hovered.bg_fill = Color32::from_rgb(56, 61, 100);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(56, 61, 100);
    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(10.0, 8.0);
    style.spacing.button_padding = Vec2::new(14.0, 7.0);
    style.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(24.0));
    style.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
    style.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
    ctx.set_style(style);
}

fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::none()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, CARD_LINE))
        .rounding(10.0)
        .inner_margin(14.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn chip(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::none()
        .fill(color.linear_multiply(0.25))
        .stroke(Stroke::new(1.0_f32, color))
        .rounding(20.0)
        .inner_margin(egui::Margin::symmetric(8.0, 2.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(color));
        });
}

fn accent_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).strong().color(Color32::WHITE)).fill(Color32::from_rgb(214, 84, 48))
}

fn fmt_duration(secs: i64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 { format!("{h}h {m:02}m") } else if m > 0 { format!("{m}m {s:02}s") } else { format!("{s}s") }
}

fn fmt_time(unix: i64) -> String {
    chrono::DateTime::from_timestamp(unix, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%b %d, %H:%M").to_string())
        .unwrap_or_else(|| "-".into())
}

fn render_body(ui: &mut egui::Ui, body: &str) {
    for line in body.lines() {
        let t = line.trim_end();
        if t.is_empty() {
            ui.add_space(4.0);
        } else if let Some(rest) = t.strip_prefix("- ") {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("•").color(ACCENT));
                ui.label(rest);
            });
        } else {
            ui.label(t);
        }
    }
}

impl App {
    fn new(paths: Paths, mode: Mode, logo: egui::TextureHandle) -> Self {
        let settings = Settings::load(&paths);
        let (tx, rx) = mpsc::channel();
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as usize)
            .unwrap_or(0);
        let launch_window = matches!(mode, Mode::Launch(_));
        let feed = news::load_local(&paths);
        let mut app = Self {
            launch_window,
            packs: mods::list_packs(&paths),
            paths,
            settings,
            tab: Tab::Home,
            last_tab: Tab::Home,
            tx,
            rx,
            logo,
            trivia_seed: seed,
            job: None,
            job_visible: false,
            feed,
            news_status: "Using saved news".into(),
            news_highlight: BTreeSet::new(),
            suppress_modal: false,
            latest: None,
            latest_status: String::new(),
            new_pack: String::new(),
            confirm_delete: None,
            new_flag_name: String::new(),
            new_flag_value: String::new(),
            live: None,
            history: Vec::new(),
            last_live: Instant::now() - Duration::from_secs(10),
            toast: None,
            confirm_uninstall: false,
        };
        match mode {
            Mode::Launch(uri) => app.start_job(Action::Launch(uri)),
            Mode::Main => {
                app.refresh_news();
                app.check_latest();
            }
        }
        app
    }

    // ------------------------------------------------------------ plumbing

    fn say(&mut self, text: impl Into<String>, ok: bool) {
        self.toast = Some((text.into(), ok, Instant::now()));
    }

    fn say_result(&mut self, r: anyhow::Result<String>) {
        match r {
            Ok(m) => self.say(m, true),
            Err(e) => self.say(format!("{e:#}"), false),
        }
    }

    fn save(&mut self) {
        if let Err(e) = self.settings.save(&self.paths) {
            self.say(format!("Could not save settings: {e:#}"), false);
        }
    }

    fn refresh_news(&mut self) {
        let tx = self.tx.clone();
        let paths = self.paths.clone();
        self.news_status = "Checking for news...".into();
        std::thread::spawn(move || {
            let _ = tx.send(UiMsg::News(news::fetch(&paths).map_err(|e| format!("{e:#}"))));
        });
    }

    fn check_latest(&mut self) {
        let tx = self.tx.clone();
        self.latest_status = "Checking...".into();
        std::thread::spawn(move || {
            let r = roblox::http().and_then(|c| roblox::latest_version(&c)).map_err(|e| format!("{e:#}"));
            let _ = tx.send(UiMsg::Latest(r));
        });
    }

    fn start_job(&mut self, action: Action) {
        if self.job.as_ref().map(|j| j.active()).unwrap_or(false) {
            return;
        }
        let label = match &action {
            Action::Launch(_) => "Launching Roblox",
            Action::Install { force: true } => "Repairing Roblox",
            Action::Install { .. } => "Updating Roblox",
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let tx = self.tx.clone();
        let emit: roblox::Emit = Arc::new(move |e| {
            let _ = tx.send(UiMsg::Launch(e));
        });
        let handle = launcher::spawn(self.paths.clone(), self.settings.clone(), action, emit, cancel.clone());
        if let Ok(mut g) = BACKGROUND.lock() {
            *g = Some(handle);
        }
        self.job = Some(Job {
            label,
            status: "Starting...".into(),
            done: 0,
            total: 0,
            started: Instant::now(),
            error: None,
            finished: false,
            launched: false,
            cancelled: false,
            close_at: None,
            cancel,
        });
        self.job_visible = true;
    }

    fn unread(&self) -> Vec<news::Post> {
        self.feed.posts.iter().filter(|p| !self.settings.seen_news.contains(&p.id)).cloned().collect()
    }

    fn mark_news_seen(&mut self) {
        let unread = self.unread();
        for p in unread {
            self.news_highlight.insert(p.id.clone());
            self.settings.seen_news.insert(p.id);
        }
        self.save();
    }

    fn update_available(&self) -> bool {
        match (&self.latest, &self.settings.installed_version) {
            (Some(l), Some(i)) => &l.upload != i,
            (Some(_), None) => true,
            _ => false,
        }
    }

    fn pump(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                UiMsg::News(Ok(feed)) => {
                    let before = self.unread().len();
                    self.feed = feed;
                    self.news_status = "Up to date".into();
                    if self.unread().len() > before {
                        self.suppress_modal = false;
                    }
                }
                UiMsg::News(Err(e)) => self.news_status = format!("Offline - showing saved news ({e})"),
                UiMsg::Latest(Ok(v)) => {
                    self.latest_status = format!("Latest Roblox: {}", v.version);
                    self.latest = Some(v);
                }
                UiMsg::Latest(Err(e)) => self.latest_status = format!("Could not check: {e}"),
                UiMsg::Launch(ev) => self.on_event(ev),
            }
        }
        if let Some(job) = &self.job {
            if self.launch_window {
                if let Some(t) = job.close_at {
                    if Instant::now() >= t {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                if job.cancelled {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if let Some((_, _, at)) = &self.toast {
            if at.elapsed() > Duration::from_secs(7) {
                self.toast = None;
            }
        }
    }

    fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Installed { guid, version } => {
                self.settings.installed_version = Some(guid);
                self.settings.installed_label = Some(version);
                self.save();
                self.check_latest();
            }
            other => {
                let launch_window = self.launch_window;
                if let Some(job) = self.job.as_mut() {
                    match other {
                        Event::Status(s) => job.status = s,
                        Event::Progress { done, total } => {
                            job.done = done;
                            job.total = total;
                        }
                        Event::Launched => {
                            job.launched = true;
                            job.status = "Roblox is starting. Have fun!".into();
                            if launch_window {
                                job.close_at = Some(Instant::now() + Duration::from_millis(1500));
                            }
                        }
                        Event::Finished => {
                            job.finished = true;
                            job.status = "Roblox is up to date.".into();
                        }
                        Event::Failed(e) => job.error = Some(e),
                        Event::Cancelled => {
                            job.cancelled = true;
                        }
                        Event::Installed { .. } => {}
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ screens

    fn job_view(&mut self, ui: &mut egui::Ui) {
        let Some(job) = self.job.as_ref() else { return };
        let (label, status, done, total) = (job.label, job.status.clone(), job.done, job.total);
        let (error, active, launched, finished) = (job.error.clone(), job.active(), job.launched, job.finished);
        let elapsed = job.started.elapsed().as_secs() as usize;
        let cancel = job.cancel.clone();

        ui.vertical_centered(|ui| {
            ui.add_space(if self.launch_window { 18.0 } else { 70.0 });
            ui.add(egui::Image::new(egui::load::SizedTexture::new(self.logo.id(), Vec2::splat(84.0))));
            ui.add_space(6.0);
            ui.label(RichText::new(branding::APP_NAME).size(26.0).strong());
            ui.add_space(10.0);
            match &error {
                Some(e) => {
                    ui.label(RichText::new("Something went wrong").size(18.0).strong().color(Color32::from_rgb(255, 110, 110)));
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(e.as_str()).color(Color32::from_rgb(235, 200, 200))).wrap());
                    });
                }
                None => {
                    ui.label(RichText::new(label).size(18.0));
                    ui.label(RichText::new(status).weak());
                    if active {
                        ui.add_space(6.0);
                        let frac = if total > 0 { (done as f32 / total as f32).clamp(0.0, 1.0) } else { 0.0 };
                        let bar = egui::ProgressBar::new(frac).desired_width(ui.available_width().min(380.0));
                        ui.add(if total > 0 { bar.show_percentage() } else { bar.animate(true) });
                        if total > 0 {
                            ui.label(
                                RichText::new(format!("{:.1} MB of {:.1} MB", done as f64 / 1.048_576e6, total as f64 / 1.048_576e6))
                                    .weak()
                                    .size(13.0),
                            );
                        }
                    }
                }
            }
            if active && self.settings.show_trivia && !branding::TRIVIA.is_empty() {
                ui.add_space(14.0);
                let i = (elapsed / 7 + self.trivia_seed) % branding::TRIVIA.len();
                egui::Frame::none().fill(CARD).rounding(10.0).inner_margin(12.0).show(ui, |ui| {
                    ui.set_max_width(400.0);
                    ui.label(RichText::new("Did you know?").strong().color(ACCENT));
                    ui.add(egui::Label::new(branding::TRIVIA[i]).wrap());
                });
            }
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let w = ui.available_width();
                ui.add_space((w - if active { 100.0 } else { 220.0 }).max(0.0) / 2.0);
                if active {
                    if ui.button("Cancel").clicked() {
                        cancel.store(true, Ordering::Relaxed);
                    }
                } else if error.is_some() {
                    if ui.button("Copy error").clicked() {
                        if let Some(e) = &error {
                            ui.ctx().copy_text(e.clone());
                        }
                    }
                    if self.launch_window {
                        if ui.button("Close").clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    } else if ui.button("Back").clicked() {
                        self.job_visible = false;
                    }
                } else if (launched || finished) && !self.launch_window && ui.button("Back").clicked() {
                    self.job_visible = false;
                }
            });
        });
    }

    fn launch_screen(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| self.job_view(ui));
    }

    fn main_screen(&mut self, ctx: &egui::Context) {
        self.handle_drops(ctx);

        egui::SidePanel::left("nav").exact_width(206.0).show(ctx, |ui| {
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.add(egui::Image::new(egui::load::SizedTexture::new(self.logo.id(), Vec2::splat(40.0))));
                ui.vertical(|ui| {
                    ui.label(RichText::new(branding::APP_NAME).size(19.0).strong());
                    ui.label(RichText::new(format!("v{}", branding::APP_VERSION)).weak().size(12.0));
                });
            });
            ui.add_space(14.0);
            let unread = self.unread().len();
            let items: [(Tab, &str); 7] = [
                (Tab::Home, "Home"),
                (Tab::News, "News"),
                (Tab::Mods, "Mods"),
                (Tab::Settings, "Settings"),
                (Tab::Activity, "Activity"),
                (Tab::Tools, "Tools"),
                (Tab::About, "About"),
            ];
            for (tab, name) in items {
                let text = if tab == Tab::News && unread > 0 { format!("{name}   ({unread} new)") } else { name.to_string() };
                let selected = self.tab == tab;
                let rich = RichText::new(text).size(16.0);
                let resp = ui.add_sized([ui.available_width(), 34.0], egui::SelectableLabel::new(selected, rich));
                if resp.clicked() {
                    self.tab = tab;
                    self.job_visible = false;
                }
            }
            ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                ui.add_space(8.0);
                let label = self.settings.installed_label.clone().unwrap_or_else(|| "not installed".into());
                ui.label(RichText::new(format!("Roblox: {label}")).weak().size(12.0));
            });
        });

        if self.tab == Tab::News && self.last_tab != Tab::News {
            self.news_highlight.clear();
            self.mark_news_seen();
        }
        if self.tab == Tab::Mods && self.last_tab != Tab::Mods {
            self.packs = mods::list_packs(&self.paths);
        }
        self.last_tab = self.tab;

        if let Some((text, ok, _)) = self.toast.clone() {
            egui::TopBottomPanel::bottom("toast").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let color = if ok { Color32::from_rgb(120, 220, 150) } else { Color32::from_rgb(255, 120, 120) };
                    ui.label(RichText::new(if ok { "OK" } else { "Error" }).strong().color(color));
                    ui.add(egui::Label::new(text).wrap());
                });
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.job.is_some() && self.job_visible {
                self.job_view(ui);
                return;
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(6.0);
                match self.tab {
                    Tab::Home => self.home_tab(ui),
                    Tab::News => self.news_tab(ui),
                    Tab::Mods => self.mods_tab(ui),
                    Tab::Settings => self.settings_tab(ui),
                    Tab::Activity => self.activity_tab(ui),
                    Tab::Tools => self.tools_tab(ui),
                    Tab::About => self.about_tab(ui),
                }
                ui.add_space(20.0);
            });
        });

        self.whats_new_window(ctx);
    }

    fn whats_new_window(&mut self, ctx: &egui::Context) {
        let unread = self.unread();
        if unread.is_empty() || self.suppress_modal || self.tab == Tab::News {
            return;
        }
        let mut got_it = false;
        egui::Window::new("What's new")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.label(format!("{} update(s) you have not seen yet:", unread.len()));
                ui.add_space(4.0);
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for post in &unread {
                        card(ui, |ui| {
                            ui.horizontal(|ui| {
                                if !post.tag.is_empty() {
                                    chip(ui, &post.tag, ACCENT);
                                }
                                ui.label(RichText::new(&post.date).weak());
                            });
                            ui.label(RichText::new(&post.title).size(18.0).strong());
                            render_body(ui, &post.body);
                        });
                    }
                });
                ui.add_space(6.0);
                if ui.add(accent_button("Got it")).clicked() {
                    got_it = true;
                }
            });
        if got_it {
            self.suppress_modal = true;
            self.mark_news_seen();
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        for f in dropped {
            let Some(path) = f.path else { continue };
            if path.extension().map(|e| e.eq_ignore_ascii_case("zip")).unwrap_or(false) {
                match mods::import_zip(&self.paths, &path) {
                    Ok(name) => {
                        self.say(format!("Imported mod pack '{name}'."), true);
                        self.tab = Tab::Mods;
                        self.packs = mods::list_packs(&self.paths);
                    }
                    Err(e) => self.say(format!("Import failed: {e:#}"), false),
                }
            } else {
                self.say("Only .zip mod packs can be imported.", false);
            }
        }
    }

    // ------------------------------------------------------------ tabs

    fn home_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ready to play");
        ui.add_space(4.0);

        if let (Some(latest), Some(url)) = (self.feed.latest_launcher.clone(), self.feed.download_url.clone()) {
            if news::is_newer(&latest, branding::APP_VERSION) {
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("RustBlox {latest} is available (you have {}).", branding::APP_VERSION)).strong());
                        if ui.add(accent_button("Download")).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                        }
                    });
                });
                ui.add_space(4.0);
            }
        }

        card(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(6.0);
                let busy = self.job.as_ref().map(|j| j.active()).unwrap_or(false);
                let btn = egui::Button::new(RichText::new("Launch Roblox").size(24.0).strong().color(Color32::WHITE))
                    .fill(Color32::from_rgb(214, 84, 48))
                    .min_size(Vec2::new(300.0, 60.0));
                if ui.add_enabled(!busy, btn).clicked() {
                    self.start_job(Action::Launch(None));
                }
                ui.add_space(6.0);
                let installed = self.settings.installed_label.clone().unwrap_or_else(|| "not installed yet".into());
                ui.label(RichText::new(format!("Installed: {installed}")).weak());
                if !self.latest_status.is_empty() {
                    ui.label(RichText::new(self.latest_status.clone()).weak());
                }
                ui.horizontal(|ui| {
                    let w = ui.available_width();
                    ui.add_space((w - 330.0).max(0.0) / 2.0);
                    if ui.button("Check for updates").clicked() {
                        self.check_latest();
                    }
                    if self.update_available() && ui.add(accent_button("Update now")).clicked() {
                        self.start_job(Action::Install { force: false });
                    }
                });
                ui.label(RichText::new("Roblox is always updated automatically when you launch.").weak().size(12.0));
            });
        });

        ui.add_space(6.0);
        card(ui, |ui| {
            ui.label(RichText::new("FPS cap").strong());
            let current = match self.settings.fps() {
                Some(v) if v >= 9999 => "Uncapped".to_string(),
                Some(v) => format!("{v} FPS"),
                None => "Roblox default".to_string(),
            };
            ui.label(RichText::new(format!("Current: {current}")).weak());
            self.fps_presets(ui);
        });

        ui.add_space(6.0);
        let unread = self.unread().len();
        if let Some(first) = self.feed.posts.first().cloned() {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Latest news").strong());
                    if unread > 0 {
                        chip(ui, &format!("{unread} unread"), ACCENT);
                    }
                });
                ui.label(RichText::new(&first.title).size(17.0));
                if ui.button("Open news").clicked() {
                    self.tab = Tab::News;
                }
            });
        }
    }

    fn fps_presets(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for v in [30u64, 60, 120, 144, 165, 240] {
                if ui.selectable_label(self.settings.fps() == Some(v), format!("{v}")).clicked() {
                    self.settings.set_fps(Some(v));
                    self.save();
                }
            }
            if ui.selectable_label(self.settings.fps().map(|v| v >= 9999).unwrap_or(false), "Uncapped").clicked() {
                self.settings.set_fps(Some(9999));
                self.save();
            }
            if ui.selectable_label(self.settings.fps().is_none(), "Default").clicked() {
                self.settings.set_fps(None);
                self.save();
            }
        });
    }

    fn news_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("News");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Refresh").clicked() {
                    self.refresh_news();
                }
                ui.label(RichText::new(self.news_status.clone()).weak().size(12.0));
            });
        });
        ui.label(RichText::new("Every post stays here, including ones from before you installed or while you were away.").weak());
        ui.add_space(6.0);
        let posts = self.feed.posts.clone();
        if posts.is_empty() {
            ui.label("No news yet.");
        }
        let unseen: Vec<String> = self.unread().into_iter().map(|p| p.id).collect();
        for post in posts {
            let is_new = self.news_highlight.contains(&post.id) || unseen.contains(&post.id);
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    if post.pinned {
                        chip(ui, "Pinned", Color32::from_rgb(120, 180, 255));
                    }
                    if !post.tag.is_empty() {
                        chip(ui, &post.tag, Color32::from_rgb(150, 150, 200));
                    }
                    if is_new {
                        chip(ui, "NEW", ACCENT);
                    }
                    ui.label(RichText::new(&post.date).weak());
                });
                ui.label(RichText::new(&post.title).size(19.0).strong());
                ui.add_space(2.0);
                render_body(ui, &post.body);
                if let Some(link) = &post.link {
                    if !link.is_empty() {
                        ui.hyperlink_to("Read more", link);
                    }
                }
            });
            ui.add_space(4.0);
        }
    }

    fn mods_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mods");
        ui.label("Each folder in your Mods directory is a mod pack with its own switch. Drag a .zip onto this window to import one.");
        ui.label(
            RichText::new("Packs apply in alphabetical order (name them 01_, 02_ for priority). Turning one off restores the original files on your next launch.")
                .weak(),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.new_pack).hint_text("New pack name").desired_width(220.0));
            if ui.button("Create pack").clicked() {
                let name = self.new_pack.clone();
                match mods::create_pack(&self.paths, &name) {
                    Ok(()) => {
                        self.new_pack.clear();
                        self.say(format!("Created pack '{}'.", name.trim()), true);
                        self.packs = mods::list_packs(&self.paths);
                    }
                    Err(e) => self.say(format!("{e:#}"), false),
                }
            }
            if ui.button("Open Mods folder").clicked() {
                if let Err(e) = tools::open_folder(&self.paths.mods()) {
                    self.say(format!("{e:#}"), false);
                }
            }
            if ui.button("Refresh").clicked() {
                self.packs = mods::list_packs(&self.paths);
            }
        });
        ui.add_space(6.0);

        if self.packs.is_empty() {
            card(ui, |ui| {
                ui.label("No mod packs yet. Create one, or drop a .zip here.");
                ui.label(RichText::new("Layout inside a pack: content/, PlatformContent/, ExtraContent/ or shaders/").weak());
            });
        }
        let packs = self.packs.clone();
        for pack in packs {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut enabled = !self.settings.disabled_mods.contains(&pack.name);
                    if ui.checkbox(&mut enabled, RichText::new(&pack.name).strong().size(16.0)).changed() {
                        if enabled {
                            self.settings.disabled_mods.remove(&pack.name);
                        } else {
                            self.settings.disabled_mods.insert(pack.name.clone());
                        }
                        self.save();
                    }
                    ui.label(RichText::new(format!("{} file(s)", pack.files)).weak());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.confirm_delete.as_deref() == Some(pack.name.as_str()) {
                            if ui.button("Cancel").clicked() {
                                self.confirm_delete = None;
                            }
                            if ui.add(accent_button("Yes, delete")).clicked() {
                                let r = mods::delete_pack(&self.paths, &pack.name).map(|_| format!("Deleted '{}'.", pack.name));
                                self.say_result(r);
                                self.confirm_delete = None;
                                self.packs = mods::list_packs(&self.paths);
                            }
                        } else {
                            if ui.button("Delete").clicked() {
                                self.confirm_delete = Some(pack.name.clone());
                            }
                            if ui.button("Open").clicked() {
                                if let Err(e) = tools::open_folder(&self.paths.mods().join(&pack.name)) {
                                    self.say(format!("{e:#}"), false);
                                }
                            }
                        }
                    });
                });
                if !pack.ignored.is_empty() {
                    ui.label(
                        RichText::new(format!("Ignored (not a Roblox folder): {}", pack.ignored.join(", ")))
                            .color(Color32::from_rgb(230, 190, 90))
                            .size(13.0),
                    );
                }
                if pack.files == 0 && pack.ignored.is_empty() {
                    ui.label(RichText::new("Empty pack. Put files in content/, PlatformContent/, ExtraContent/ or shaders/.").weak());
                }
            });
            ui.add_space(4.0);
        }
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.add_space(4.0);

        card(ui, |ui| {
            ui.label(RichText::new("FPS cap").strong());
            let mut fps = self.settings.fps().unwrap_or(60);
            ui.horizontal(|ui| {
                ui.label("Custom value");
                if ui.add(egui::DragValue::new(&mut fps).range(1..=10000).speed(1)).changed() {
                    self.settings.set_fps(Some(fps));
                    self.save();
                }
            });
            self.fps_presets(ui);
            ui.label(RichText::new(format!("Stored as the FastFlag {FPS_FLAG}. Applied on the next launch.")).weak().size(12.0));
        });

        ui.add_space(6.0);
        card(ui, |ui| {
            ui.label(RichText::new("Launch").strong());
            let mut trivia = self.settings.show_trivia;
            if ui.checkbox(&mut trivia, "Show \"Did you know?\" tips while downloading").changed() {
                self.settings.show_trivia = trivia;
                self.save();
            }
            ui.horizontal(|ui| {
                if ui.button("Set RustBlox as the Roblox launcher").clicked() {
                    let r = tools::register(&self.paths).map(|_| "RustBlox now opens Roblox links.".to_string());
                    self.say_result(r);
                }
                if ui.button("Restore the previous launcher").clicked() {
                    let r = tools::unregister(&self.paths).map(|_| "Previous launcher restored.".to_string());
                    self.say_result(r);
                }
            });
        });

        ui.add_space(6.0);
        card(ui, |ui| {
            ui.label(RichText::new("Activity").strong());
            let mut tracking = self.settings.activity_tracking;
            if ui.checkbox(&mut tracking, "Track the server I am in and keep a play history").changed() {
                self.settings.activity_tracking = tracking;
                self.save();
            }
            let mut loc = self.settings.server_location;
            if ui.checkbox(&mut loc, "Look up the server region (sends the server's IP address to ipinfo.io)").changed() {
                self.settings.server_location = loc;
                self.save();
            }
        });

        ui.add_space(6.0);
        card(ui, |ui| {
            ui.label(RichText::new("Discord Rich Presence").strong());
            if branding::DISCORD_APP_ID.is_empty() {
                ui.label(
                    RichText::new("Disabled in this build: set DISCORD_APP_ID in src/branding.rs to enable it.")
                        .color(Color32::from_rgb(230, 190, 90)),
                );
            }
            let mut rpc = self.settings.discord_rpc;
            if ui.checkbox(&mut rpc, "Show what I am playing on Discord").changed() {
                self.settings.discord_rpc = rpc;
                self.save();
            }
            let mut show = self.settings.rpc_show_game;
            if ui.checkbox(&mut show, "Include the game name and server region").changed() {
                self.settings.rpc_show_game = show;
                self.save();
            }
            let mut join = self.settings.rpc_join_button;
            if ui.checkbox(&mut join, "Add a \"Join server\" button (anyone who sees it can join your server)").changed() {
                self.settings.rpc_join_button = join;
                self.save();
            }
        });

        ui.add_space(6.0);
        card(ui, |ui| {
            ui.label(RichText::new("FastFlags").strong());
            ui.label(RichText::new("Roblox ignores flags outside its allow-list, so some flags will have no effect.").weak().size(12.0));
            let flags: Vec<(String, serde_json::Value)> =
                self.settings.fflags.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            let mut remove: Option<String> = None;
            egui::Grid::new("flags").num_columns(3).striped(true).show(ui, |ui| {
                for (k, v) in &flags {
                    ui.label(k);
                    ui.label(RichText::new(v.to_string()).monospace());
                    if ui.small_button("Remove").clicked() {
                        remove = Some(k.clone());
                    }
                    ui.end_row();
                }
            });
            if let Some(k) = remove {
                self.settings.fflags.remove(&k);
                self.save();
            }
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_flag_name).hint_text("FFlagName").desired_width(260.0));
                ui.add(egui::TextEdit::singleline(&mut self.new_flag_value).hint_text("value").desired_width(110.0));
                if ui.button("Add flag").clicked() {
                    let name = self.new_flag_name.trim().to_string();
                    if !config::valid_flag(&name) {
                        self.say(
                            format!("'{name}' is not a valid FastFlag name (prefixes: {}).", config::FLAG_PREFIXES.join(", ")),
                            false,
                        );
                    } else if self.new_flag_value.trim().is_empty() {
                        self.say("Enter a value (true, false, a number or text).", false);
                    } else {
                        self.settings.fflags.insert(name.clone(), config::parse_value(&self.new_flag_value));
                        self.new_flag_name.clear();
                        self.new_flag_value.clear();
                        self.save();
                        self.say(format!("Set {name}."), true);
                    }
                }
            });
        });
    }

    fn activity_tab(&mut self, ui: &mut egui::Ui) {
        if self.last_live.elapsed() > Duration::from_secs(1) {
            self.live = activity::read_live(&self.paths);
            self.history = activity::load_history(&self.paths);
            self.last_live = Instant::now();
        }
        ui.heading("Activity");
        if !self.settings.activity_tracking {
            ui.label(RichText::new("Activity tracking is off. Turn it on in Settings.").color(Color32::from_rgb(230, 190, 90)));
        }
        ui.add_space(4.0);
        card(ui, |ui| {
            ui.label(RichText::new("Current server").strong());
            match self.live.clone() {
                Some(s) if s.joined => {
                    ui.label(RichText::new(s.game_name.clone().unwrap_or_else(|| format!("Place {}", s.place_id))).size(19.0).strong());
                    if let Some(c) = &s.creator {
                        ui.label(RichText::new(format!("by {c}")).weak());
                    }
                    egui::Grid::new("live").num_columns(2).show(ui, |ui| {
                        ui.label("Region");
                        ui.label(s.location.clone().unwrap_or_else(|| "looking up...".into()));
                        ui.end_row();
                        ui.label("Server IP");
                        ui.label(&s.server_ip);
                        ui.end_row();
                        ui.label("Place ID");
                        ui.label(s.place_id.to_string());
                        ui.end_row();
                        ui.label("Job ID");
                        ui.label(RichText::new(&s.job_id).monospace());
                        ui.end_row();
                        ui.label("Time here");
                        ui.label(fmt_duration(activity::now_unix() - s.started_unix));
                        ui.end_row();
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Copy join link").clicked() {
                            ui.ctx().copy_text(s.web_link());
                        }
                        if ui.button("Copy roblox:// link").clicked() {
                            ui.ctx().copy_text(s.deep_link());
                        }
                        if ui.button("Copy Job ID").clicked() {
                            ui.ctx().copy_text(s.job_id.clone());
                        }
                    });
                }
                Some(_) => {
                    ui.label("Joining a server...");
                }
                None => {
                    ui.label(RichText::new("Not in an experience. Launch Roblox with RustBlox and join a game to see it here.").weak());
                }
            }
        });
        ui.add_space(6.0);
        card(ui, |ui| {
            ui.label(RichText::new("History").strong());
            if self.history.is_empty() {
                ui.label(RichText::new("Nothing yet.").weak());
            } else {
                egui::Grid::new("history").num_columns(4).striped(true).show(ui, |ui| {
                    ui.label(RichText::new("When").strong());
                    ui.label(RichText::new("Experience").strong());
                    ui.label(RichText::new("Region").strong());
                    ui.label(RichText::new("Time").strong());
                    ui.end_row();
                    for h in self.history.iter().rev().take(50) {
                        ui.label(fmt_time(h.started_unix));
                        ui.label(&h.game_name);
                        ui.label(&h.location);
                        ui.label(fmt_duration(h.duration_secs));
                        ui.end_row();
                    }
                });
            }
        });
    }

    fn tools_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Tools");
        ui.add_space(4.0);
        card(ui, |ui| {
            ui.label(RichText::new("Client").strong());
            ui.horizontal_wrapped(|ui| {
                if ui.button("Repair (reinstall Roblox)").clicked() {
                    self.start_job(Action::Install { force: true });
                }
                if ui.button("Close all Roblox windows").clicked() {
                    let r = tools::kill_roblox();
                    self.say_result(r);
                }
                if self.confirm_uninstall {
                    if ui.add(accent_button("Yes, remove the client")).clicked() {
                        let r = tools::uninstall_client(&self.paths);
                        if r.is_ok() {
                            self.settings.installed_version = None;
                            self.settings.installed_label = None;
                            self.save();
                        }
                        self.say_result(r);
                        self.confirm_uninstall = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_uninstall = false;
                    }
                } else if ui.button("Remove installed client").clicked() {
                    self.confirm_uninstall = true;
                }
            });
        });
        ui.add_space(6.0);
        card(ui, |ui| {
            ui.label(RichText::new("Clean up").strong());
            ui.horizontal_wrapped(|ui| {
                if ui.button("Clear Roblox cache").clicked() {
                    let r = tools::clear_roblox_cache();
                    self.say_result(r);
                }
                if ui.button("Clear Roblox logs").clicked() {
                    let r = tools::clear_roblox_logs();
                    self.say_result(r);
                }
                if ui.button("Reset all FastFlags").clicked() {
                    self.settings.fflags.clear();
                    self.save();
                    self.say("All FastFlags removed.", true);
                }
            });
        });
        ui.add_space(6.0);
        card(ui, |ui| {
            ui.label(RichText::new("Folders").strong());
            ui.horizontal_wrapped(|ui| {
                let targets: [(&str, std::path::PathBuf); 3] = [
                    ("RustBlox data", self.paths.root.clone()),
                    ("Mods", self.paths.mods()),
                    ("Roblox logs", roblox_logs_dir().unwrap_or_else(|| self.paths.logs())),
                ];
                for (name, path) in targets {
                    if ui.button(name).clicked() {
                        if let Err(e) = tools::open_folder(&path) {
                            self.say(format!("{e:#}"), false);
                        }
                    }
                }
            });
        });
        ui.add_space(6.0);
        card(ui, |ui| {
            ui.label(RichText::new("Support").strong());
            ui.label(RichText::new("Bundles launcher logs, your settings and the tail of the Roblox log. Lines mentioning tickets, tokens or cookies are removed, but read it before sharing.").weak().size(13.0));
            if ui.button("Export diagnostics").clicked() {
                match tools::export_diagnostics(&self.paths, &self.settings) {
                    Ok(path) => {
                        self.say(format!("Saved {}", path.display()), true);
                        if let Some(dir) = path.parent() {
                            let _ = tools::open_folder(dir);
                        }
                    }
                    Err(e) => self.say(format!("{e:#}"), false),
                }
            }
        });
    }

    fn about_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(egui::Image::new(egui::load::SizedTexture::new(self.logo.id(), Vec2::splat(72.0))));
            ui.vertical(|ui| {
                ui.label(RichText::new(branding::APP_NAME).size(28.0).strong());
                ui.label(RichText::new(format!("Version {}", branding::APP_VERSION)).weak());
                ui.label(branding::TAGLINE);
            });
        });
        ui.add_space(8.0);
        card(ui, |ui| {
            ui.label(RichText::new(format!("Made by {}", branding::AUTHOR)).size(18.0).strong());
            chip(ui, "Non-profit, free forever", Color32::from_rgb(120, 220, 150));
            ui.add_space(4.0);
            for line in branding::MISSION {
                ui.add(egui::Label::new(*line).wrap());
                ui.add_space(2.0);
            }
        });
        ui.add_space(6.0);
        card(ui, |ui| {
            ui.label(RichText::new("What we are aiming for").strong());
            for g in branding::GOALS {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("•").color(ACCENT));
                    ui.label(*g);
                });
            }
        });
        ui.add_space(6.0);
        card(ui, |ui| {
            ui.label(RichText::new("What RustBlox connects to").strong());
            for n in branding::NETWORK_NOTES {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("•").color(ACCENT));
                    ui.label(*n);
                });
            }
            ui.label(RichText::new("RustBlox never asks for your Roblox password and never reads your account cookie.").weak());
        });
        ui.add_space(6.0);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                if !branding::SOURCE_URL.is_empty() {
                    ui.hyperlink_to("Source code", branding::SOURCE_URL);
                }
                if !branding::DISCORD_URL.is_empty() {
                    ui.hyperlink_to("Discord", branding::DISCORD_URL);
                }
            });
            ui.label(RichText::new(branding::DISCLAIMER).weak().size(12.5));
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump(ctx);
        ctx.request_repaint_after(Duration::from_millis(250));
        if self.launch_window {
            self.launch_screen(ctx);
        } else {
            self.main_screen(ctx);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(job) = &self.job {
            if job.active() {
                // Window closed during a download: cancel, and do not wait for the worker.
                job.cancel.store(true, Ordering::Relaxed);
                if let Ok(mut g) = BACKGROUND.lock() {
                    g.take();
                }
            }
        }
    }
}
