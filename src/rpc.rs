//! Discord Rich Presence over the local Discord IPC socket.

use crate::activity::Session;
use crate::branding;
use crate::config::Settings;
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

pub struct Rpc {
    app_id: String,
    client: Option<DiscordIpcClient>,
    last_key: String,
    next_try: Instant,
}

impl Rpc {
    pub fn new(app_id: &str) -> Self {
        Self { app_id: app_id.to_string(), client: None, last_key: String::new(), next_try: Instant::now() }
    }

    fn connect(&mut self) {
        if self.client.is_some() || Instant::now() < self.next_try {
            return;
        }
        self.next_try = Instant::now() + Duration::from_secs(15);
        match DiscordIpcClient::new(&self.app_id) {
            Ok(mut c) => match c.connect() {
                Ok(()) => {
                    debug!("connected to Discord");
                    self.client = Some(c);
                    self.last_key.clear();
                }
                Err(e) => debug!("Discord not available: {e}"),
            },
            Err(e) => warn!("Discord client error: {e}"),
        }
    }

    pub fn update(&mut self, session: &Option<Session>, s: &Settings) {
        self.connect();
        let key = match session {
            Some(x) if x.joined => format!("{}|{}|{:?}|{:?}|{}|{}", x.place_id, x.job_id, x.game_name, x.location, s.rpc_show_game, s.rpc_join_button),
            _ => format!("idle|{}", s.rpc_show_game),
        };
        if key == self.last_key {
            return;
        }
        let Some(client) = self.client.as_mut() else { return };

        let (details, state, start, link): (String, String, i64, Option<String>) = match session {
            Some(x) if x.joined && s.rpc_show_game => (
                x.game_name.clone().map(|n| format!("Playing {n}")).unwrap_or_else(|| "Playing Roblox".into()),
                x.location.clone().map(|l| format!("Server: {l}")).unwrap_or_else(|| "In an experience".into()),
                x.started_unix,
                if s.rpc_join_button { Some(x.web_link()) } else { None },
            ),
            Some(x) if x.joined => ("Playing Roblox".into(), "In an experience".into(), x.started_unix, None),
            _ => ("Roblox is open".into(), "Browsing".into(), crate::activity::now_unix(), None),
        };

        let large_text = format!("{} v{}", branding::APP_NAME, branding::APP_VERSION);
        let mut act = activity::Activity::new()
            .details(&details)
            .state(&state)
            .timestamps(activity::Timestamps::new().start(start))
            .assets(activity::Assets::new().large_image("logo").large_text(&large_text));
        if let Some(url) = link.as_deref() {
            act = act.buttons(vec![activity::Button::new("Join server", url)]);
        }
        match client.set_activity(act) {
            Ok(()) => self.last_key = key,
            Err(e) => {
                debug!("Discord update failed, will reconnect: {e}");
                self.client = None;
                self.next_try = Instant::now() + Duration::from_secs(5);
            }
        }
    }

    pub fn close(&mut self) {
        if let Some(mut c) = self.client.take() {
            let _ = c.clear_activity();
            let _ = c.close();
        }
    }
}
