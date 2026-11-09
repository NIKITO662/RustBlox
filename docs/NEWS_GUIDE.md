# News guide

The launcher shows a news feed that lives in one file, `news.json`, in a public GitHub repo
(suggested name: `rustblox-news`). Editing that file and committing is how you publish.
Nobody has to update the launcher to see it.

## Setup (once)
1. Create a public repo, e.g. `ItsNiki66/rustblox-news`, and add `news.json` (copy `assets/news.json`).
2. In `src/branding.rs`, set `NEWS_URL` to
   `https://raw.githubusercontent.com/ItsNiki66/rustblox-news/main/news.json`
3. Rebuild and release the launcher.

## How "nobody is left behind" works
- The launcher merges the posts bundled in the app, the ones cached on the PC and the live feed, by `id`.
  A post is never dropped, even if you later delete it from the file.
- Every post a user has not seen is listed in a "What's new" window when they open the launcher,
  not just the latest one. Someone who skips five versions sees all five.
- Offline users still see everything they have downloaded before.

## Post format
```json
{
  "latest_launcher": "0.3.0",
  "download_url": "https://github.com/ItsNiki66/RustBlox/releases/latest",
  "posts": [
    {
      "id": "2026-12-01-v030",
      "date": "2026-12-01",
      "title": "RustBlox 0.3.0",
      "tag": "Release",
      "pinned": false,
      "link": "https://github.com/ItsNiki66/RustBlox/releases/tag/v0.3.0",
      "body": "Short intro line.\n\nWhat's new:\n- First change\n- Second change\n\nFixes:\n- Something that was broken"
    }
  ]
}
```
- `id`: unique, never reused, never edited. Use `YYYY-MM-DD-short-name`.
- `date`: `YYYY-MM-DD`. Newest first; pinned posts stay on top.
- `tag`: Release, Announcement, Guide, Maintenance, Incident.
- `body`: plain text. Blank line = paragraph break. A line starting with `- ` becomes a bullet.
- `latest_launcher` + `download_url`: when `latest_launcher` is newer than the installed version, Home shows an update banner.

## Who writes it and when
- Owner: you (ItsNiki66). Add co-writers as repo collaborators; use pull requests so a second person reads every post.
- Publish a post for: every release, any known issue or outage, any change to what data the launcher touches, and big community news.
- Minimum cadence: one post per release. A short "Known issue" post is better than silence.
- Fixing a mistake: add a new post ("Correction") instead of editing history, so everyone who saw the old one sees the fix.

## Writing rules
- Title under 80 characters, say what changed, not "Update".
- Lead with what the player gets. Mention how to use it in one line.
- Be honest about problems and timelines. No hype, no promises you cannot keep.
- Never include secrets, personal data or anything you would not post publicly.

## Before you publish
```
python tools/validate_news.py path/to/news.json
```
