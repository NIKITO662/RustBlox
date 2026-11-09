//! News feed. Posts are merged from the bundled copy, the local cache and the live feed by id,
//! and are never dropped, so nobody misses a post by being offline or skipping versions.

use crate::branding;
use crate::paths::Paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

const BUNDLED: &str = include_str!("../assets/news.json");

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Post {
    pub id: String,
    pub date: String,
    pub title: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub link: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Feed {
    #[serde(default)]
    pub latest_launcher: Option<String>,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub posts: Vec<Post>,
}

fn parse(text: &str) -> Result<Feed> {
    serde_json::from_str(text).context("news feed is not valid JSON")
}

fn merge(feeds: Vec<Feed>) -> Feed {
    // Later feeds win, so the order is: bundled, cache, live.
    let mut posts: BTreeMap<String, Post> = BTreeMap::new();
    let mut out = Feed::default();
    for f in feeds {
        if f.latest_launcher.is_some() {
            out.latest_launcher = f.latest_launcher;
        }
        if f.download_url.is_some() {
            out.download_url = f.download_url;
        }
        for post in f.posts {
            posts.insert(post.id.clone(), post);
        }
    }
    let mut list: Vec<Post> = posts.into_values().collect();
    list.sort_by(|a, b| b.pinned.cmp(&a.pinned).then_with(|| b.date.cmp(&a.date)).then_with(|| b.id.cmp(&a.id)));
    out.posts = list;
    out
}

fn cached(p: &Paths) -> Option<Feed> {
    fs::read_to_string(p.news_cache()).ok().and_then(|t| parse(&t).ok())
}

/// Instant, offline-safe view: bundled posts plus whatever was cached earlier.
pub fn load_local(p: &Paths) -> Feed {
    let mut feeds = Vec::new();
    if let Ok(f) = parse(BUNDLED) {
        feeds.push(f);
    }
    if let Some(f) = cached(p) {
        feeds.push(f);
    }
    merge(feeds)
}

pub fn fetch(p: &Paths) -> Result<Feed> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(branding::USER_AGENT)
        .timeout(Duration::from_secs(12))
        .build()?;
    let text = client
        .get(branding::NEWS_URL)
        .send()?
        .error_for_status()
        .context("news feed unavailable")?
        .text()?;
    let live = parse(&text)?;
    let merged = {
        let mut feeds = Vec::new();
        if let Ok(f) = parse(BUNDLED) {
            feeds.push(f);
        }
        if let Some(f) = cached(p) {
            feeds.push(f);
        }
        feeds.push(live);
        merge(feeds)
    };
    // The cache keeps every post ever seen, even if it later disappears from the live feed.
    fs::write(p.news_cache(), serde_json::to_string_pretty(&merged)?)?;
    Ok(merged)
}

/// True when `a` is a newer dotted version than `b`.
pub fn is_newer(a: &str, b: &str) -> bool {
    let nums = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v').split('.').map(|x| x.parse::<u64>().unwrap_or(0)).collect()
    };
    let (x, y) = (nums(a), nums(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (x.get(i).copied().unwrap_or(0), y.get(i).copied().unwrap_or(0));
        if p != q {
            return p > q;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(id: &str, date: &str, pinned: bool) -> Post {
        Post { id: id.into(), date: date.into(), title: id.into(), tag: String::new(), body: String::new(), pinned, link: None }
    }

    #[test]
    fn merge_keeps_every_post_and_orders_them() {
        let a = Feed { posts: vec![post("old", "2026-01-01", false), post("pin", "2026-01-02", true)], ..Default::default() };
        let b = Feed { posts: vec![post("new", "2026-06-01", false)], latest_launcher: Some("0.3.0".into()), ..Default::default() };
        let m = merge(vec![a, b]);
        let ids: Vec<&str> = m.posts.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["pin", "new", "old"]);
        assert_eq!(m.latest_launcher.as_deref(), Some("0.3.0"));
    }

    #[test]
    fn later_feed_overrides_same_id() {
        let mut p = post("x", "2026-01-01", false);
        let a = Feed { posts: vec![p.clone()], ..Default::default() };
        p.title = "edited".into();
        let b = Feed { posts: vec![p], ..Default::default() };
        assert_eq!(merge(vec![a, b]).posts[0].title, "edited");
    }

    #[test]
    fn bundled_news_is_valid() {
        let f = parse(BUNDLED).expect("bundled news.json");
        assert!(!f.posts.is_empty());
    }

    #[test]
    fn version_comparison() {
        assert!(is_newer("0.3.0", "0.2.9"));
        assert!(is_newer("v1.0", "0.9.9"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
    }
}
