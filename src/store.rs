//! Local accounts ("profiles"): subscriptions and watch history, saved as JSON.
//! Everything stays on this machine; no sign-in with a third party is needed.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::yt::VideoItem;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Sub {
    pub id: String,
    pub name: String,
    pub avatar: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct HistItem {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub channel_id: String,
    pub length: String,
    pub watched_at: u64,
    /// Last playback position in seconds, used to resume.
    pub pos: f64,
    pub duration: f64,
}

impl HistItem {
    pub fn to_video(&self) -> VideoItem {
        VideoItem {
            id: self.id.clone(),
            title: self.title.clone(),
            channel: self.channel.clone(),
            channel_id: self.channel_id.clone(),
            views: String::new(),
            published: String::new(),
            length: self.length.clone(),
            thumb: format!("https://i.ytimg.com/vi/{}/mqdefault.jpg", self.id),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Profile {
    pub name: String,
    pub subs: Vec<Sub>,
    pub history: Vec<HistItem>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Store {
    pub active: usize,
    pub profiles: Vec<Profile>,
}

impl Default for Store {
    fn default() -> Self {
        Store {
            active: 0,
            profiles: vec![Profile {
                name: "Me".into(),
                ..Default::default()
            }],
        }
    }
}

fn data_path() -> PathBuf {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.unwrap_or_else(|| PathBuf::from("."))
        .join("RustTube")
        .join("profiles.json")
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Store {
    pub fn load() -> Store {
        let mut s: Store = std::fs::read(data_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if s.profiles.is_empty() {
            s = Store::default();
        }
        s.active = s.active.min(s.profiles.len() - 1);
        s
    }

    pub fn save(&self) {
        let path = data_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            // Write then rename so a crash never leaves a half-written file.
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }

    pub fn me(&self) -> &Profile {
        &self.profiles[self.active]
    }

    pub fn me_mut(&mut self) -> &mut Profile {
        &mut self.profiles[self.active]
    }

    pub fn is_subscribed(&self, channel_id: &str) -> bool {
        self.me().subs.iter().any(|s| s.id == channel_id)
    }

    pub fn toggle_sub(&mut self, sub: Sub) {
        let p = self.me_mut();
        if let Some(i) = p.subs.iter().position(|s| s.id == sub.id) {
            p.subs.remove(i);
        } else {
            p.subs.insert(0, sub);
        }
        self.save();
    }

    pub fn add_profile(&mut self, name: &str) {
        self.profiles.push(Profile {
            name: name.trim().to_string(),
            ..Default::default()
        });
        self.active = self.profiles.len() - 1;
        self.save();
    }

    pub fn delete_active(&mut self) {
        if self.profiles.len() > 1 {
            self.profiles.remove(self.active);
            self.active = self.active.min(self.profiles.len() - 1);
            self.save();
        }
    }

    /// Record a watch (moves it to the front, keeps the last 500).
    pub fn record_watch(&mut self, item: HistItem) {
        let p = self.me_mut();
        let old_pos = p.history.iter().find(|h| h.id == item.id).map(|h| h.pos);
        p.history.retain(|h| h.id != item.id);
        let mut item = item;
        if let Some(pos) = old_pos {
            item.pos = pos;
        }
        p.history.insert(0, item);
        p.history.truncate(500);
        self.save();
    }

    pub fn resume_pos(&self, id: &str) -> f64 {
        self.me()
            .history
            .iter()
            .find(|h| h.id == id)
            .filter(|h| h.pos > 10.0 && (h.duration <= 0.0 || h.pos < h.duration - 15.0))
            .map(|h| h.pos)
            .unwrap_or(0.0)
    }

    pub fn set_progress(&mut self, id: &str, pos: f64, duration: f64) {
        if let Some(h) = self.me_mut().history.iter_mut().find(|h| h.id == id) {
            h.pos = pos;
            if duration > 0.0 {
                h.duration = duration;
                if h.length.is_empty() {
                    let t = duration as u64;
                    h.length = if t >= 3600 {
                        format!("{}:{:02}:{:02}", t / 3600, (t / 60) % 60, t % 60)
                    } else {
                        format!("{}:{:02}", t / 60, t % 60)
                    };
                }
            }
        }
    }

    pub fn clear_history(&mut self) {
        self.me_mut().history.clear();
        self.save();
    }
}
