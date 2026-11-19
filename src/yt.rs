//! Minimal YouTube (InnerTube) client: search, channels, watch-page data,
//! comments and playable stream URLs. All calls are blocking; the UI runs
//! them on worker threads.

use serde_json::{json, Value};

const WEB_VERSION: &str = "2.20250101.00.00";

#[derive(Clone, Debug, Default)]
pub struct VideoItem {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub channel_id: String,
    pub views: String,
    pub published: String,
    pub length: String,
    pub thumb: String,
}

#[derive(Clone, Debug, Default)]
pub struct Comment {
    pub author: String,
    pub avatar: String,
    pub text: String,
    pub when: String,
    pub likes: String,
    pub replies: String,
}

#[derive(Clone, Debug, Default)]
pub struct WatchInfo {
    pub title: String,
    pub channel: String,
    pub channel_id: String,
    pub channel_avatar: String,
    pub views: String,
    pub description: String,
    pub related: Vec<VideoItem>,
    pub comments_token: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ChannelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub avatar: String,
    pub videos: Vec<VideoItem>,
}

type Res<T> = Result<T, String>;

fn web_ctx() -> Value {
    json!({"client": {"clientName": "WEB", "clientVersion": WEB_VERSION, "hl": "en", "gl": "US"}})
}

fn post(endpoint: &str, body: Value, ua: Option<&str>) -> Res<Value> {
    let url = format!("https://www.youtube.com/youtubei/v1/{endpoint}?prettyPrint=false");
    let mut req = ureq::post(&url).header("Content-Type", "application/json");
    if let Some(ua) = ua {
        req = req.header("User-Agent", ua);
    }
    let mut resp = req.send_json(body).map_err(|e| format!("network error: {e}"))?;
    resp.body_mut()
        .read_json::<Value>()
        .map_err(|e| format!("bad response: {e}"))
}

// ---------- JSON helpers ----------

fn text_of(v: &Value) -> String {
    if let Some(s) = v.get("simpleText").and_then(Value::as_str) {
        return s.to_string();
    }
    if let Some(s) = v.get("content").and_then(Value::as_str) {
        return s.to_string();
    }
    if let Some(runs) = v.get("runs").and_then(Value::as_array) {
        return runs
            .iter()
            .filter_map(|r| r.get("text").and_then(Value::as_str))
            .collect();
    }
    String::new()
}

fn last_thumb(v: &Value) -> String {
    v.get("thumbnails")
        .and_then(Value::as_array)
        .and_then(|a| a.last())
        .and_then(|t| t.get("url"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => {
            if let Some(x) = m.get(key) {
                return Some(x);
            }
            m.values().find_map(|x| find_key(x, key))
        }
        Value::Array(a) => a.iter().find_map(|x| find_key(x, key)),
        _ => None,
    }
}

fn collect_key<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == key {
                    out.push(x);
                } else {
                    collect_key(x, key, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_key(x, key, out)),
        _ => {}
    }
}

fn first_str(v: &Value, key: &str) -> String {
    find_key(v, key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Collect every video we recognise in a response, in document order.
fn parse_videos(root: &Value) -> Vec<VideoItem> {
    let mut out = Vec::new();
    walk_videos(root, &mut out);
    out
}

fn walk_videos(v: &Value, out: &mut Vec<VideoItem>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                match k.as_str() {
                    "videoRenderer" | "compactVideoRenderer" | "gridVideoRenderer" => {
                        if let Some(item) = video_from_renderer(x) {
                            out.push(item);
                        }
                    }
                    "lockupViewModel" => {
                        if let Some(item) = video_from_lockup(x) {
                            out.push(item);
                        } else {
                            walk_videos(x, out);
                        }
                    }
                    _ => walk_videos(x, out),
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| walk_videos(x, out)),
        _ => {}
    }
}

fn video_from_renderer(r: &Value) -> Option<VideoItem> {
    let id = r.get("videoId")?.as_str()?.to_string();
    let byline = r
        .get("longBylineText")
        .or_else(|| r.get("ownerText"))
        .or_else(|| r.get("shortBylineText"));
    Some(VideoItem {
        thumb: format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg"),
        id,
        title: r.get("title").map(text_of).unwrap_or_default(),
        channel: byline.map(text_of).unwrap_or_default(),
        channel_id: byline.map(|b| first_str(b, "browseId")).unwrap_or_default(),
        views: r
            .get("viewCountText")
            .or_else(|| r.get("shortViewCountText"))
            .map(text_of)
            .unwrap_or_default(),
        published: r.get("publishedTimeText").map(text_of).unwrap_or_default(),
        length: r.get("lengthText").map(text_of).unwrap_or_default(),
    })
}

fn video_from_lockup(l: &Value) -> Option<VideoItem> {
    if l.get("contentType")?.as_str()? != "LOCKUP_CONTENT_TYPE_VIDEO" {
        return None;
    }
    let id = l.get("contentId")?.as_str()?.to_string();
    let meta = l.get("metadata")?.get("lockupMetadataViewModel")?;
    let rows = meta
        .pointer("/metadata/contentMetadataViewModel/metadataRows")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let row_parts = |i: usize| -> Vec<String> {
        rows.get(i)
            .and_then(|r| r.get("metadataParts"))
            .and_then(Value::as_array)
            .map(|p| {
                p.iter()
                    .filter_map(|x| x.get("text").map(text_of))
                    .collect()
            })
            .unwrap_or_default()
    };
    let channel = row_parts(0).into_iter().next().unwrap_or_default();
    let stats = row_parts(1);
    let length = find_key(l, "thumbnailBadgeViewModel")
        .and_then(|b| b.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Some(VideoItem {
        thumb: format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg"),
        id,
        title: meta.get("title").map(text_of).unwrap_or_default(),
        channel,
        channel_id: first_str(meta, "browseId"),
        views: stats.first().cloned().unwrap_or_default(),
        published: stats.get(1).cloned().unwrap_or_default(),
        length,
    })
}

// ---------- public API ----------

pub fn search(query: &str) -> Res<Vec<VideoItem>> {
    let v = post("search", json!({"context": web_ctx(), "query": query}), None)?;
    Ok(parse_videos(&v))
}

pub fn channel(id: &str) -> Res<ChannelInfo> {
    // "EgZ2aWRlb3PyBgQKAjoA" selects the Videos tab.
    let v = post(
        "browse",
        json!({"context": web_ctx(), "browseId": id, "params": "EgZ2aWRlb3PyBgQKAjoA"}),
        None,
    )?;
    let meta = find_key(&v, "channelMetadataRenderer");
    let get = |k: &str| -> String {
        meta.and_then(|m| m.get(k))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let avatar = meta
        .and_then(|m| m.get("avatar"))
        .map(last_thumb)
        .unwrap_or_default();
    let name = get("title");
    let mut videos = parse_videos(&v);
    for vid in &mut videos {
        if vid.channel.is_empty() {
            vid.channel = name.clone();
        }
        if vid.channel_id.is_empty() {
            vid.channel_id = id.to_string();
        }
    }
    Ok(ChannelInfo {
        id: id.to_string(),
        name,
        description: get("description"),
        avatar,
        videos,
    })
}

pub fn watch_info(video_id: &str) -> Res<WatchInfo> {
    let v = post("next", json!({"context": web_ctx(), "videoId": video_id}), None)?;
    let primary = find_key(&v, "videoPrimaryInfoRenderer");
    let secondary = find_key(&v, "videoSecondaryInfoRenderer");
    let owner = secondary.and_then(|s| find_key(s, "videoOwnerRenderer"));

    let mut related: Vec<VideoItem> = parse_videos(&v);
    related.retain(|r| r.id != video_id);

    let mut comments_token = None;
    let mut sections = Vec::new();
    collect_key(&v, "itemSectionRenderer", &mut sections);
    for s in sections {
        if s.get("sectionIdentifier").and_then(Value::as_str) == Some("comment-item-section") {
            comments_token = find_key(s, "token").and_then(Value::as_str).map(String::from);
            break;
        }
    }

    Ok(WatchInfo {
        title: primary.and_then(|p| p.get("title")).map(text_of).unwrap_or_default(),
        channel: owner.and_then(|o| o.get("title")).map(text_of).unwrap_or_default(),
        channel_id: owner.map(|o| first_str(o, "browseId")).unwrap_or_default(),
        channel_avatar: owner
            .and_then(|o| o.get("thumbnail"))
            .map(last_thumb)
            .unwrap_or_default(),
        views: primary
            .and_then(|p| find_key(p, "viewCount"))
            .map(text_of)
            .unwrap_or_default(),
        description: secondary
            .and_then(|s| s.get("attributedDescription"))
            .map(text_of)
            .unwrap_or_default(),
        related,
        comments_token,
    })
}

pub fn comments(token: &str) -> Res<(Vec<Comment>, Option<String>)> {
    let v = post("next", json!({"context": web_ctx(), "continuation": token}), None)?;
    let mut payloads = Vec::new();
    collect_key(&v, "commentEntityPayload", &mut payloads);
    let list = payloads
        .iter()
        .map(|p| Comment {
            author: p.pointer("/author/displayName").and_then(Value::as_str).unwrap_or("").into(),
            avatar: p.pointer("/author/avatarThumbnailUrl").and_then(Value::as_str).unwrap_or("").into(),
            text: p.pointer("/properties/content/content").and_then(Value::as_str).unwrap_or("").into(),
            when: p.pointer("/properties/publishedTime").and_then(Value::as_str).unwrap_or("").into(),
            likes: p.pointer("/toolbar/likeCountNotliked").and_then(Value::as_str).unwrap_or("").into(),
            replies: p.pointer("/toolbar/replyCount").and_then(Value::as_str).unwrap_or("").into(),
        })
        .filter(|c| !c.text.is_empty())
        .collect();

    // Next-page token: the last continuationItemRenderer in the response.
    let mut conts = Vec::new();
    collect_key(&v, "continuationItemRenderer", &mut conts);
    let next = conts
        .iter()
        .rev()
        .find_map(|c| find_key(c, "token").and_then(Value::as_str))
        .map(String::from);
    Ok((list, next))
}

/// Fetch raw bytes (thumbnails, avatars).
pub fn fetch_bytes(url: &str) -> Res<Vec<u8>> {
    let mut resp = ureq::get(url).call().map_err(|e| e.to_string())?;
    resp.body_mut().read_to_vec().map_err(|e| e.to_string())
}
