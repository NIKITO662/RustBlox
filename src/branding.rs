//! Everything you may want to edit before publishing lives here.

pub const APP_NAME: &str = "RustBlox";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const AUTHOR: &str = "ItsNiki66";
pub const USER_AGENT: &str = concat!("RustBlox/", env!("CARGO_PKG_VERSION"));

/// Where the news feed is hosted. Create a public GitHub repo, put `news.json` in it,
/// and point this at the raw file. See docs/NEWS_GUIDE.md.
pub const NEWS_URL: &str = "https://raw.githubusercontent.com/NIKITO662/rustblox-news/main/news.json";

/// Discord Rich Presence application ID (https://discord.com/developers/applications).
/// Leave empty to disable Rich Presence. Upload an art asset named `logo` for the large image.
pub const DISCORD_APP_ID: &str = "1569320913084747936";

/// Optional links shown on the About page. Empty string hides the button.
pub const SOURCE_URL: &str = "";
pub const DISCORD_URL: &str = "";

pub const TAGLINE: &str = "A free, community-made launcher for Roblox.";

pub const MISSION: &[&str] = &[
    "RustBlox is a non-profit, community project made by ItsNiki66. It has no ads, no paywalls and no tracking, and it never asks for money.",
    "It exists because playing Roblox should be fast, transparent and in your control: you decide how your client runs, which mods it loads, and what information leaves your computer.",
];

pub const GOALS: &[&str] = &[
    "Always stay current: Roblox is checked for updates on every launch, so you are never left on an outdated client.",
    "Mods that are safe to try: every mod pack has its own switch, and turning one off restores the original files.",
    "Know what is happening: see your server, region and play history without digging through log files.",
    "Nobody is left behind: every news post stays available, including the ones you missed while away.",
    "Stay open and honest: the About page lists exactly what the launcher connects to.",
];

pub const DISCLAIMER: &str = "RustBlox is an independent fan project. It is not made, endorsed or supported by Roblox Corporation. Roblox is a trademark of Roblox Corporation.";

pub const NETWORK_NOTES: &[&str] = &[
    "Roblox servers (clientsettingscdn.roblox.com, setup.rbxcdn.com) to check for and download the client.",
    "The RustBlox news feed (a JSON file on GitHub) to show news.",
    "Roblox public APIs, to look up the name of the game you are playing (for Activity and Discord).",
    "ipinfo.io, to show the region of your game server. Optional: Settings > Activity.",
    "Discord, locally on your computer, for Rich Presence. Optional: Settings > Discord.",
];

pub const TRIVIA: &[&str] = &[
    "Each folder inside your Mods directory is its own mod pack with an on/off switch.",
    "Mod packs are applied in alphabetical order. Name them 01_, 02_ to control which wins.",
    "Turning a mod pack off restores the original Roblox files on your next launch.",
    "Your FPS cap is a FastFlag named DFIntTaskSchedulerTargetFps. You can change it in Settings.",
    "Press F9 in an experience to open the developer console and see client logs.",
    "Press F11 in an experience to toggle fullscreen.",
    "Roblox experiences are scripted in Luau, a language derived from Lua.",
    "The Activity tab shows your current server and keeps a history of the games you played.",
    "Tools > Export diagnostics bundles logs for bug reports, with sensitive lines removed.",
    "RustBlox checks for a new Roblox version every time you launch, so you never fall behind.",
    "Drag a .zip mod pack onto the Mods tab to import it in one step.",
    "Roblox was released to the public in 2006.",
];
