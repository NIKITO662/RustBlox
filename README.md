# RustBlox

A free, community-made third-party Roblox launcher for Windows, by ItsNiki66.
Not affiliated with Roblox Corporation.

Features: automatic Roblox updates on every launch, installer, mod packs with per-pack switches
(originals are backed up and restored), FPS cap and FastFlag editor, news feed that never skips posts,
"Did you know?" tips while downloading, activity tracking (server, region, play history),
Discord Rich Presence, QoL tools (repair, cache and log cleanup, diagnostics export), About page.

## For players
Download `RustBlox-Setup-x.y.z.exe` from the Releases page and run it. No admin rights needed.

## For you: getting a release out (no local C++ tools needed)
1. Create a GitHub repo and push this folder.
2. Tag a release: `git tag v0.2.0 && git push --tags`.
3. The workflow in `.github/workflows/release.yml` runs the tests, builds `rustblox.exe` on Windows,
   builds the installer and attaches `RustBlox-Setup-0.2.0.exe` plus `SHA256SUMS.txt` to the release.

Building locally instead: install Rust (rustup.rs) and "Build Tools for Visual Studio" with the
"Desktop development with C++" workload (this fixes `link.exe not found`), then run `build.bat`.
For the installer also install Inno Setup 6.

When you bump the version, change it in `Cargo.toml` and `installer/setup.iss`.

## Things to set before publishing
- `src/branding.rs`: `NEWS_URL` (see docs/NEWS_GUIDE.md), `DISCORD_APP_ID`, `SOURCE_URL`, `DISCORD_URL`,
  and the About text (`MISSION`, `GOALS`).
- Discord Rich Presence: create an app at discord.com/developers, copy its Application ID into
  `DISCORD_APP_ID` (already set to 1569320913084747936), and upload an art asset named `logo` (use `assets/logo.png`).
- Name: `RustBlox` is a working name. It is set in `branding.rs`, `Cargo.toml`, `installer/setup.iss`
  and `build.rs`. Check that your final name does not imitate Roblox's trademarks.

## Trust and distribution
An unsigned installer triggers Windows SmartScreen warnings, and with many users that hurts.
- Publish the source, keep the checksums file, and describe network use on the About page (already included).
- Code signing removes most warnings. Options: SignPath Foundation (free for open source),
  Azure Trusted Signing, or a standard code-signing certificate.

## Layout
- `src/` Rust source (roblox.rs install/launch, mods.rs, activity.rs, rpc.rs, news.rs, tools.rs, ui.rs)
- `assets/` logo (PNG, ICO, SVG) and bundled news. Regenerate the logo with `python tools/make_logo.py`.
- `installer/setup.iss` Inno Setup script
- `docs/NEWS_GUIDE.md` how to write and publish news
