//! Personalised feeds built from a profile's subscriptions and watch history.

use std::collections::{HashMap, HashSet};

use crate::store::{HistItem, Sub};
use crate::yt::{self, VideoItem};

/// Round-robin merge of several ranked lists, dropping duplicates and anything in `skip`.
fn interleave(lists: Vec<Vec<VideoItem>>, skip: &HashSet<String>, cap: usize) -> Vec<VideoItem> {
    let mut seen = skip.clone();
    let mut out = Vec::new();
    let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
    for i in 0..longest {
        for l in &lists {
            if let Some(v) = l.get(i) {
                if seen.insert(v.id.clone()) {
                    out.push(v.clone());
                    if out.len() >= cap {
                        return out;
                    }
                }
            }
        }
    }
    out
}

fn channel_videos(subs: &[Sub], per: usize) -> Vec<Vec<VideoItem>> {
    std::thread::scope(|s| {
        let handles: Vec<_> = subs
            .iter()
            .map(|sub| {
                s.spawn(move || {
                    yt::channel(&sub.id)
                        .map(|c| c.videos.into_iter().take(per).collect::<Vec<_>>())
                        .unwrap_or_default()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap_or_default()).collect()
    })
}

/// Latest uploads from subscribed channels.
pub fn subscriptions(subs: &[Sub]) -> Vec<VideoItem> {
    let subs: Vec<Sub> = subs.iter().take(20).cloned().collect();
    interleave(channel_videos(&subs, 6), &HashSet::new(), 120)
}

/// "For You": videos related to recent watches, fresh uploads from channels the
/// user subscribes to or watches often, mixed together and never repeating
/// something already watched.
pub fn for_you(history: &[HistItem], subs: &[Sub]) -> Result<Vec<VideoItem>, String> {
    let watched: HashSet<String> = history.iter().map(|h| h.id.clone()).collect();

    // Most-watched channels in recent history.
    let mut freq: HashMap<&str, (usize, &HistItem)> = HashMap::new();
    for h in history.iter().take(60).filter(|h| !h.channel_id.is_empty()) {
        freq.entry(h.channel_id.as_str()).or_insert((0, h)).0 += 1;
    }
    let mut top: Vec<_> = freq.into_values().collect();
    top.sort_by(|a, b| b.0.cmp(&a.0));
    let fav_channels: Vec<Sub> = top
        .iter()
        .take(4)
        .map(|(_, h)| Sub {
            id: h.channel_id.clone(),
            name: h.channel.clone(),
            avatar: String::new(),
        })
        .collect();

    let seeds: Vec<&HistItem> = history.iter().take(5).collect();
    let sub_slice: Vec<Sub> = subs.iter().take(8).cloned().collect();

    let (related, from_subs, from_favs) = std::thread::scope(|s| {
        let rel: Vec<_> = seeds
            .iter()
            .map(|h| {
                let id = h.id.clone();
                s.spawn(move || {
                    yt::watch_info(&id)
                        .map(|w| w.related.into_iter().take(12).collect::<Vec<_>>())
                        .unwrap_or_default()
                })
            })
            .collect();
        let subs_h = s.spawn(|| channel_videos(&sub_slice, 3));
        let favs_h = s.spawn(|| channel_videos(&fav_channels, 3));
        (
            rel.into_iter().map(|h| h.join().unwrap_or_default()).collect::<Vec<_>>(),
            subs_h.join().unwrap_or_default(),
            favs_h.join().unwrap_or_default(),
        )
    });

    // Order matters for round-robin: subscriptions first, then related to the
    // most recent watches, then favourite channels.
    let mut lists = Vec::new();
    lists.extend(from_subs);
    lists.extend(related);
    lists.extend(from_favs);

    let mut out = interleave(lists, &watched, 60);
    if out.is_empty() {
        out = yt::search("trending today")?;
    }
    Ok(out)
}
