//! YouTube's official embedded player, hosted in a native web view that sits
//! on top of the egui window at the position of the video rectangle.

use std::sync::{Arc, Mutex};

use eframe::egui;
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{Rect, WebView, WebViewBuilder};

use crate::embed;

#[derive(Clone, Copy, Default, Debug)]
pub struct Status {
    pub pos: f64,
    pub dur: f64,
    /// YT.PlayerState: -1 unstarted, 0 ended, 1 playing, 2 paused, 3 buffering, 5 cued
    pub state: i32,
    /// YouTube player error code (0 = none). 101/150: embedding disabled.
    pub error: i64,
}

pub struct WebPlayer {
    view: WebView,
    status: Arc<Mutex<Status>>,
    visible: bool,
    last: Option<egui::Rect>,
}

fn bounds(r: egui::Rect) -> Rect {
    Rect {
        position: LogicalPosition::new(r.min.x as f64, r.min.y as f64).into(),
        size: LogicalSize::new(r.width().max(1.0) as f64, r.height().max(1.0) as f64).into(),
    }
}

impl WebPlayer {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        embed::start();
        let status = Arc::new(Mutex::new(Status::default()));
        let st = status.clone();
        let ctx = cc.egui_ctx.clone();
        let view = WebViewBuilder::new()
            .with_bounds(bounds(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(2.0, 2.0))))
            .with_visible(false)
            .with_background_color((0, 0, 0, 255))
            .with_autoplay(true)
            .with_ipc_handler(move |req| {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(req.body()) {
                    let s = Status {
                        pos: v["t"].as_f64().unwrap_or(0.0),
                        dur: v["d"].as_f64().unwrap_or(0.0),
                        state: v["s"].as_i64().unwrap_or(-1) as i32,
                        error: v["e"].as_i64().unwrap_or(0),
                    };
                    if let Ok(mut g) = st.lock() {
                        *g = s;
                    }
                    ctx.request_repaint();
                }
            })
            .with_url("about:blank")
            .build_as_child(cc)
            .map_err(|e| format!("could not start the web view: {e}"))?;
        Ok(WebPlayer { view, status, visible: false, last: None })
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|g| *g).unwrap_or_default()
    }

    pub fn load(&mut self, video_id: &str, start: f64) {
        if let Ok(mut g) = self.status.lock() {
            *g = Status { state: -1, ..Default::default() };
        }
        let _ = self.view.load_url(&embed::url(video_id, start));
    }

    pub fn stop(&mut self) {
        let _ = self.view.load_url("about:blank");
        if let Ok(mut g) = self.status.lock() {
            *g = Status::default();
        }
        self.hide();
    }

    pub fn hide(&mut self) {
        if self.visible {
            let _ = self.view.set_visible(false);
            self.visible = false;
        }
    }

    /// Place the player over `rect` (egui points) and make sure it is visible.
    pub fn show_at(&mut self, rect: egui::Rect) {
        if self.last != Some(rect) {
            let _ = self.view.set_bounds(bounds(rect));
            self.last = Some(rect);
        }
        if !self.visible {
            let _ = self.view.set_visible(true);
            self.visible = true;
        }
    }
}
