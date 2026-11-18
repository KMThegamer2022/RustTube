#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod player;
mod proxy;
mod rec;
mod store;
mod yt;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};

use eframe::egui::{self, Color32, RichText};
use player::Player;
use store::{HistItem, Store, Sub};
use yt::{ChannelInfo, Comment, Streams, VideoItem, WatchInfo};

const RED: Color32 = Color32::from_rgb(255, 48, 48);
const CATEGORIES: [(&str, &str); 8] = [
    ("Trending", "trending today"),
    ("Music", "music videos new"),
    ("Gaming", "gaming highlights"),
    ("Tech", "tech news review"),
    ("Science", "science explained"),
    ("Sports", "sports highlights"),
    ("News", "world news today"),
    ("Movies", "movie trailers"),
];

#[derive(Clone, PartialEq)]
enum Page {
    Home(usize),
    ForYou,
    Subs,
    History,
    Search(String),
    Watch(String),
    Channel(String),
}

enum Msg {
    Results(u64, Result<Vec<VideoItem>, String>),
    Channel(u64, Result<ChannelInfo, String>),
    Watch(String, Result<WatchInfo, String>),
    Streams(String, Result<Streams, String>),
    Comments(String, Result<(Vec<Comment>, Option<String>), String>),
    Thumb(String, Option<egui::ColorImage>),
}

#[derive(Default)]
struct WatchState {
    id: String,
    info: Option<WatchInfo>,
    streams: Option<Streams>,
    error: Option<String>,
    quality: usize,
    comments: Vec<Comment>,
    comments_next: Option<String>,
    comments_loading: bool,
    desc_open: bool,
    speed: f64,
    recorded: bool,
}

struct App {
    player: Option<Player>,
    player_err: Option<String>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,

    history: Vec<Page>,
    cursor: usize,
    search_box: String,
    request_gen: u64,

    results: Vec<VideoItem>,
    channel: Option<ChannelInfo>,
    loading: bool,
    error: Option<String>,

    watch: WatchState,
    fullscreen: bool,
    last_mouse_move: f64,
    last_debug: f64,
    last_progress_save: f64,
    store: Store,
    new_profile: String,
    volume: f32,

    thumbs: HashMap<String, egui::TextureHandle>,
    thumbs_pending: HashSet<String>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_style(&cc.egui_ctx);
        let (player, player_err) = match Player::new(cc) {
            Ok(p) => (Some(p), None),
            Err(e) => (None, Some(e)),
        };
        let (tx, rx) = channel();
        let mut app = App {
            player,
            player_err,
            tx,
            rx,
            history: vec![],
            cursor: 0,
            search_box: String::new(),
            request_gen: 0,
            results: vec![],
            channel: None,
            loading: false,
            error: None,
            watch: WatchState::default(),
            fullscreen: false,
            last_mouse_move: 0.0,
            last_debug: 0.0,
            last_progress_save: 0.0,
            store: Store::load(),
            new_profile: String::new(),
            volume: 100.0,
            thumbs: HashMap::new(),
            thumbs_pending: HashSet::new(),
        };
        let start = if app.store.me().history.is_empty() && app.store.me().subs.is_empty() {
            Page::Home(0)
        } else {
            Page::ForYou
        };
        app.history.push(start.clone());
        app.enter(&cc.egui_ctx, start);
        if let Ok(id) = std::env::var("RUSTTUBE_AUTOPLAY") {
            app.navigate(&cc.egui_ctx, Page::Watch(id));
        }
        app
    }

    fn page(&self) -> Page {
        self.history[self.cursor].clone()
    }

    fn navigate(&mut self, ctx: &egui::Context, page: Page) {
        if self.page() == page {
            return;
        }
        self.history.truncate(self.cursor + 1);
        self.history.push(page.clone());
        self.cursor += 1;
        self.enter(ctx, page);
    }

    fn go_back(&mut self, ctx: &egui::Context) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let p = self.page();
            self.enter(ctx, p);
        }
    }

    fn go_forward(&mut self, ctx: &egui::Context) {
        if self.cursor + 1 < self.history.len() {
            self.cursor += 1;
            let p = self.page();
            self.enter(ctx, p);
        }
    }

    /// Start loading whatever the page needs.
    fn enter(&mut self, ctx: &egui::Context, page: Page) {
        self.save_progress();
        self.request_gen += 1;
        let gen = self.request_gen;
        self.error = None;
        let tx = self.tx.clone();
        let ctx2 = ctx.clone();
        if !matches!(page, Page::Watch(_)) {
            if let Some(p) = &mut self.player {
                p.stop();
            }
            self.fullscreen = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
        match page {
            Page::Home(i) => {
                self.loading = true;
                self.results.clear();
                self.channel = None;
                let q = CATEGORIES[i].1.to_string();
                std::thread::spawn(move || {
                    let _ = tx.send(Msg::Results(gen, yt::search(&q)));
                    ctx2.request_repaint();
                });
            }
            Page::ForYou => {
                self.loading = true;
                self.results.clear();
                self.channel = None;
                let (h, subs) = (self.store.me().history.clone(), self.store.me().subs.clone());
                std::thread::spawn(move || {
                    let _ = tx.send(Msg::Results(gen, rec::for_you(&h, &subs)));
                    ctx2.request_repaint();
                });
            }
            Page::Subs => {
                self.loading = true;
                self.results.clear();
                self.channel = None;
                let subs = self.store.me().subs.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(Msg::Results(gen, Ok(rec::subscriptions(&subs))));
                    ctx2.request_repaint();
                });
            }
            Page::History => {
                self.loading = false;
                self.channel = None;
                self.results = self.store.me().history.iter().map(|h| h.to_video()).collect();
            }
            Page::Search(q) => {
                self.loading = true;
                self.results.clear();
                self.channel = None;
                self.search_box = q.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(Msg::Results(gen, yt::search(&q)));
                    ctx2.request_repaint();
                });
            }
            Page::Channel(id) => {
                self.loading = true;
                self.results.clear();
                self.channel = None;
                std::thread::spawn(move || {
                    let _ = tx.send(Msg::Channel(gen, yt::channel(&id)));
                    ctx2.request_repaint();
                });
            }
            Page::Watch(id) => {
                self.watch = WatchState {
                    id: id.clone(),
                    speed: 1.0,
                    ..Default::default()
                };
                if let Some(p) = &mut self.player {
                    p.stop();
                }
                let (tx1, ctx1, id1) = (tx.clone(), ctx.clone(), id.clone());
                std::thread::spawn(move || {
                    let _ = tx1.send(Msg::Watch(id1.clone(), yt::watch_info(&id1)));
                    ctx1.request_repaint();
                });
                std::thread::spawn(move || {
                    let _ = tx.send(Msg::Streams(id.clone(), yt::streams(&id)));
                    ctx2.request_repaint();
                });
            }
        }
    }

    fn save_progress(&mut self) {
        if self.watch.id.is_empty() {
            return;
        }
        if let Some(p) = &self.player {
            let st = p.state();
            if st.has_file && st.duration > 0.0 {
                let id = self.watch.id.clone();
                self.store.set_progress(&id, st.pos, st.duration);
                self.store.save();
            }
        }
    }

    fn record_if_ready(&mut self) {
        if self.watch.recorded {
            return;
        }
        let (Some(info), Some(s)) = (&self.watch.info, &self.watch.streams) else {
            return;
        };
        let item = HistItem {
            id: self.watch.id.clone(),
            title: info.title.clone(),
            channel: info.channel.clone(),
            channel_id: info.channel_id.clone(),
            length: fmt_time(s.duration),
            watched_at: store::now_secs(),
            pos: 0.0,
            duration: s.duration,
        };
        self.watch.recorded = true;
        self.store.record_watch(item);
    }

    fn sub_button(&mut self, ui: &mut egui::Ui, id: &str, name: &str, avatar: &str) {
        if id.is_empty() {
            return;
        }
        let subscribed = self.store.is_subscribed(id);
        let btn = if subscribed {
            egui::Button::new("✔ Subscribed")
        } else {
            egui::Button::new(RichText::new("Subscribe").strong().color(Color32::WHITE)).fill(RED)
        };
        if ui.add(btn).clicked() {
            self.store.toggle_sub(Sub {
                id: id.to_string(),
                name: name.to_string(),
                avatar: avatar.to_string(),
            });
        }
    }

    fn load_comments(&mut self, ctx: &egui::Context, token: String) {
        self.watch.comments_loading = true;
        let (tx, ctx, id) = (self.tx.clone(), ctx.clone(), self.watch.id.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Comments(id, yt::comments(&token)));
            ctx.request_repaint();
        });
    }

    fn start_stream(&mut self, start: f64) {
        let (Some(player), Some(s)) = (&mut self.player, &self.watch.streams) else {
            return;
        };
        let q = self.watch.quality.min(s.video.len().saturating_sub(1));
        let v = &s.video[q];
        let video_url = proxy::register(&v.url, v.len);
        let audio_url = proxy::register(&s.audio_url, s.audio_len);
        player.load(&video_url, &audio_url, start);
        player.set_volume(self.volume as f64);
        player.set_speed(self.watch.speed);
    }

    fn pump(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Results(gen, r) if gen == self.request_gen => {
                    self.loading = false;
                    match r {
                        Ok(v) => self.results = v,
                        Err(e) => self.error = Some(e),
                    }
                }
                Msg::Channel(gen, r) if gen == self.request_gen => {
                    self.loading = false;
                    match r {
                        Ok(c) => {
                            self.results = c.videos.clone();
                            self.channel = Some(c);
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                Msg::Watch(id, r) if id == self.watch.id => match r {
                    Ok(info) => {
                        if let Some(t) = info.comments_token.clone() {
                            self.load_comments(ctx, t);
                        }
                        self.watch.info = Some(info);
                        self.record_if_ready();
                    }
                    Err(e) => self.watch.error = Some(e),
                },
                Msg::Streams(id, r) if id == self.watch.id => match r {
                    Ok(s) => {
                        // Default to the best quality up to 1080p.
                        self.watch.quality = s.video.iter().position(|f| f.height <= 1080).unwrap_or(0);
                        self.watch.streams = Some(s);
                        let resume = self.store.resume_pos(&id);
                        self.start_stream(resume);
                        self.record_if_ready();
                    }
                    Err(e) => self.watch.error = Some(e),
                },
                Msg::Comments(id, r) if id == self.watch.id => {
                    self.watch.comments_loading = false;
                    if let Ok((mut list, next)) = r {
                        self.watch.comments.append(&mut list);
                        self.watch.comments_next = next;
                    }
                }
                Msg::Thumb(url, img) => {
                    self.thumbs_pending.remove(&url);
                    if let Some(img) = img {
                        let tex = ctx.load_texture(&url, img, egui::TextureOptions::LINEAR);
                        self.thumbs.insert(url, tex);
                    }
                }
                _ => {}
            }
        }
    }

    /// Returns a texture if loaded, otherwise kicks off a background fetch.
    fn thumb(&mut self, ctx: &egui::Context, url: &str) -> Option<egui::TextureHandle> {
        if url.is_empty() {
            return None;
        }
        if let Some(t) = self.thumbs.get(url) {
            return Some(t.clone());
        }
        if self.thumbs_pending.insert(url.to_string()) {
            let (tx, ctx, url) = (self.tx.clone(), ctx.clone(), url.to_string());
            std::thread::spawn(move || {
                let img = yt::fetch_bytes(&url)
                    .ok()
                    .and_then(|b| image::load_from_memory(&b).ok())
                    .map(|i| {
                        let i = i.to_rgba8();
                        egui::ColorImage::from_rgba_unmultiplied(
                            [i.width() as usize, i.height() as usize],
                            i.as_raw(),
                        )
                    });
                let _ = tx.send(Msg::Thumb(url, img));
                ctx.request_repaint();
            });
        }
        None
    }

    // ---------- UI pieces ----------

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(self.cursor > 0, egui::Button::new("◀")).clicked() {
                    self.go_back(ctx);
                }
                if ui
                    .add_enabled(self.cursor + 1 < self.history.len(), egui::Button::new("▶"))
                    .clicked()
                {
                    self.go_forward(ctx);
                }
                if ui
                    .add(egui::Label::new(RichText::new("▶ RustTube").color(RED).size(20.0).strong()).sense(egui::Sense::click()))
                    .clicked()
                {
                    self.navigate(ctx, Page::Home(0));
                }
                ui.add_space(12.0);
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.search_box)
                        .hint_text("Search YouTube")
                        .desired_width((ui.available_width() - 220.0).clamp(120.0, 560.0)),
                );
                let submit = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.button("🔍 Search").clicked() || submit) && !self.search_box.trim().is_empty() {
                    let q = self.search_box.trim().to_string();
                    self.navigate(ctx, Page::Search(q));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.account_menu(ui, ctx);
                });
            });
            ui.horizontal(|ui| {
                let cur = self.page();
                let tabs = [
                    ("For You", Page::ForYou),
                    ("Explore", Page::Home(0)),
                    ("Subscriptions", Page::Subs),
                    ("History", Page::History),
                ];
                for (name, p) in tabs {
                    let sel = std::mem::discriminant(&cur) == std::mem::discriminant(&p);
                    if ui.selectable_label(sel, name).clicked() {
                        self.navigate(ctx, p);
                    }
                }
            });
            ui.add_space(4.0);
        });
    }

    fn account_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let initial: String = self
            .store
            .me()
            .name
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "?".into());
        let mut changed = false;
        ui.menu_button(RichText::new(format!(" {initial} ")).strong().size(16.0), |ui| {
            ui.label(RichText::new("Profiles on this computer").weak());
            let names: Vec<String> = self.store.profiles.iter().map(|p| p.name.clone()).collect();
            for (i, n) in names.iter().enumerate() {
                if ui.selectable_label(self.store.active == i, n).clicked() {
                    self.store.active = i;
                    self.store.save();
                    changed = true;
                    ui.close_menu();
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_profile).hint_text("New profile name").desired_width(140.0));
                if ui.button("Add").clicked() && !self.new_profile.trim().is_empty() {
                    let name = self.new_profile.clone();
                    self.store.add_profile(&name);
                    self.new_profile.clear();
                    changed = true;
                    ui.close_menu();
                }
            });
            ui.separator();
            if ui.button("Clear watch history").clicked() {
                self.store.clear_history();
                changed = true;
                ui.close_menu();
            }
            if self.store.profiles.len() > 1 && ui.button("Delete this profile").clicked() {
                self.store.delete_active();
                changed = true;
                ui.close_menu();
            }
        });
        ui.label(RichText::new(self.store.me().name.clone()).weak());
        if changed {
            let p = self.page();
            self.enter(ctx, p);
        }
    }

    fn video_grid(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<Page> {
        let mut go = None;
        let avail = ui.available_width();
        let gap = 16.0;
        let cols = ((avail + gap) / (300.0 + gap)).floor().max(1.0) as usize;
        let card_w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32 - 1.0).max(120.0);
        let items = self.results.clone();
        for row in items.chunks(cols) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for v in row {
                    if let Some(p) = self.video_card(ui, ctx, v, card_w) {
                        go = Some(p);
                    }
                }
            });
            ui.add_space(14.0);
        }
        go
    }

    fn video_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, v: &VideoItem, w: f32) -> Option<Page> {
        let mut go = None;
        ui.allocate_ui_with_layout(egui::vec2(w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(w);
            ui.set_max_width(w);
            let h = w * 9.0 / 16.0;
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
            let radius = 10.0;
            match self.thumb(ctx, &v.thumb) {
                Some(t) => {
                    egui::Image::new(egui::load::SizedTexture::new(t.id(), egui::vec2(w, h)))
                        .corner_radius(radius)
                        .paint_at(ui, rect);
                }
                None => {
                    ui.painter().rect_filled(rect, radius, Color32::from_gray(40));
                }
            }
            if resp.hovered() {
                ui.painter().rect_stroke(
                    rect,
                    radius,
                    egui::Stroke::new(2.0_f32, Color32::from_white_alpha(70)),
                    egui::StrokeKind::Inside,
                );
            }
            if !v.length.is_empty() {
                let galley = ui.painter().layout_no_wrap(
                    v.length.clone(),
                    egui::FontId::proportional(12.0),
                    Color32::WHITE,
                );
                let r = egui::Rect::from_min_size(
                    rect.right_bottom() - galley.size() - egui::vec2(12.0, 10.0),
                    galley.size() + egui::vec2(8.0, 4.0),
                );
                ui.painter().rect_filled(r, 4.0, Color32::from_black_alpha(200));
                ui.painter().galley(r.min + egui::vec2(4.0, 2.0), galley, Color32::WHITE);
            }
            if resp.clicked() {
                go = Some(Page::Watch(v.id.clone()));
            }
            ui.add_space(6.0);
            let t = ui.add(
                egui::Label::new(RichText::new(&v.title).strong().size(14.5))
                    .truncate()
                    .sense(egui::Sense::click()),
            );
            if t.clicked() {
                go = Some(Page::Watch(v.id.clone()));
            }
            let ch = ui.add(
                egui::Label::new(RichText::new(&v.channel).weak().size(12.5))
                    .truncate()
                    .sense(egui::Sense::click()),
            );
            if ch.clicked() && !v.channel_id.is_empty() {
                go = Some(Page::Channel(v.channel_id.clone()));
            }
            let meta = [v.views.as_str(), v.published.as_str()]
                .iter()
                .filter(|s| !s.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" · ");
            ui.label(RichText::new(meta).weak().size(12.0));
        });
        go
    }

    fn feed_page(&mut self, ctx: &egui::Context, page: Page) {
        let mut go = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(8.0);
                match &page {
                    Page::Home(sel) => {
                        ui.horizontal_wrapped(|ui| {
                            for (i, (name, _)) in CATEGORIES.iter().enumerate() {
                                if ui.selectable_label(*sel == i, *name).clicked() {
                                    go = Some(Page::Home(i));
                                }
                            }
                        });
                    }
                    Page::ForYou => {
                        ui.heading(format!("Recommended for {}", self.store.me().name));
                        ui.label(RichText::new("Based on your subscriptions and what you watch.").weak());
                    }
                    Page::Subs => {
                        ui.heading("Subscriptions");
                        let subs = self.store.me().subs.clone();
                        if subs.is_empty() {
                            ui.label("You haven't subscribed to anyone yet. Open a video and press Subscribe.");
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                for sub in &subs {
                                    if let Some(t) = self.thumb(ctx, &sub.avatar) {
                                        ui.add(egui::Image::new(egui::load::SizedTexture::new(t.id(), egui::vec2(20.0, 20.0))).corner_radius(10.0));
                                    }
                                    if ui.link(&sub.name).clicked() {
                                        go = Some(Page::Channel(sub.id.clone()));
                                    }
                                    ui.add_space(8.0);
                                }
                            });
                        }
                    }
                    Page::History => {
                        ui.heading("Watch history");
                    }
                    Page::Search(q) => {
                        ui.heading(format!("Results for “{q}”"));
                    }
                    Page::Channel(_) => {
                        if let Some(c) = self.channel.clone() {
                            ui.horizontal(|ui| {
                                if let Some(t) = self.thumb(ctx, &c.avatar) {
                                    ui.add(
                                        egui::Image::new(egui::load::SizedTexture::new(t.id(), egui::vec2(72.0, 72.0)))
                                            .corner_radius(36.0),
                                    );
                                }
                                ui.vertical(|ui| {
                                    ui.heading(&c.name);
                                    self.sub_button(ui, &c.id, &c.name, &c.avatar);
                                    let d: String = c.description.chars().take(220).collect();
                                    ui.label(RichText::new(d).weak());
                                });
                            });
                            ui.separator();
                            ui.label(RichText::new("Videos").strong());
                        }
                    }
                    _ => {}
                }
                ui.add_space(10.0);
                if self.loading {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| {
                        ui.spinner();
                    });
                } else if let Some(e) = &self.error {
                    ui.colored_label(Color32::LIGHT_RED, format!("Error: {e}"));
                } else if self.results.is_empty() {
                    ui.label("Nothing found.");
                } else if let Some(p) = self.video_grid(ui, ctx) {
                    go = Some(p);
                }
            });
        });
        if let Some(p) = go {
            self.navigate(ctx, p);
        }
    }

    fn player_area(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, width: f32, height: f32) {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
        ui.painter().rect_filled(rect, if self.fullscreen { 0.0 } else { 10.0 }, Color32::BLACK);
        let st = self.player.as_ref().map(|p| p.state()).unwrap_or_default();
        if let Some(p) = &self.player {
            if st.has_file {
                p.paint(ui, rect);
            }
        }
        if resp.clicked() {
            if let Some(p) = &self.player {
                p.toggle_pause();
            }
        }
        if resp.double_clicked() {
            self.set_fullscreen(ctx, !self.fullscreen);
        }
        if ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) && resp.hovered() {
            self.last_mouse_move = ctx.input(|i| i.time);
        }
        let now = ctx.input(|i| i.time);
        let show_controls = !self.fullscreen || now - self.last_mouse_move < 2.5 || st.paused;

        if st.buffering || (!st.has_file && self.watch.error.is_none()) {
            ui.put(
                egui::Rect::from_center_size(rect.center(), egui::vec2(40.0, 40.0)),
                egui::Spinner::new().size(36.0),
            );
        }
        if let Some(e) = &self.watch.error {
            ui.put(
                rect.shrink(20.0),
                egui::Label::new(RichText::new(format!("Can't play this video:\n{e}")).color(Color32::LIGHT_RED)),
            );
        }
        if st.has_file && st.paused && !st.buffering {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if st.ended { "⟲" } else { "⏵" },
                egui::FontId::proportional(64.0),
                Color32::from_white_alpha(200),
            );
        }
        if show_controls && st.has_file {
            self.controls(ui, ctx, rect, st);
        }
        if self.fullscreen && !show_controls {
            ctx.set_cursor_icon(egui::CursorIcon::None);
        }
    }

    fn controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, rect: egui::Rect, st: player::PlayState) {
        let bar = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 64.0), rect.right_bottom());
        ui.painter().rect_filled(bar, 0.0, Color32::from_black_alpha(150));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(egui::vec2(12.0, 6.0))));
        let ui = &mut child;
        // Seek bar
        let mut pos = st.pos as f32;
        let dur = st.duration.max(1.0) as f32;
        ui.spacing_mut().slider_width = ui.available_width();
        let slider = ui.add(egui::Slider::new(&mut pos, 0.0..=dur).show_value(false).trailing_fill(true));
        if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
            if let Some(p) = &self.player {
                p.seek_abs(pos as f64);
            }
        }
        ui.horizontal(|ui| {
            if ui.button(if st.paused { "⏵" } else { "⏸" }).clicked() {
                if let Some(p) = &self.player {
                    p.toggle_pause();
                }
            }
            ui.label(RichText::new(format!("{} / {}", fmt_time(st.pos), fmt_time(st.duration))).color(Color32::WHITE));
            ui.add_space(8.0);
            ui.label("🔊");
            ui.spacing_mut().slider_width = 90.0;
            let mut vol = self.volume;
            if ui.add(egui::Slider::new(&mut vol, 0.0..=100.0).show_value(false)).changed() {
                self.volume = vol;
                if let Some(p) = &self.player {
                    p.set_volume(vol as f64);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(if self.fullscreen { "⛶ Exit" } else { "⛶" }).clicked() {
                    self.set_fullscreen(ctx, !self.fullscreen);
                }
                // Speed
                let mut speed = self.watch.speed;
                egui::ComboBox::from_id_salt("speed")
                    .selected_text(format!("{speed}x"))
                    .width(60.0)
                    .show_ui(ui, |ui| {
                        for s in [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0] {
                            ui.selectable_value(&mut speed, s, format!("{s}x"));
                        }
                    });
                if speed != self.watch.speed {
                    self.watch.speed = speed;
                    if let Some(p) = &self.player {
                        p.set_speed(speed);
                    }
                }
                // Quality
                if let Some(s) = &self.watch.streams {
                    let cur = self.watch.quality.min(s.video.len().saturating_sub(1));
                    let mut sel = cur;
                    egui::ComboBox::from_id_salt("quality")
                        .selected_text(format!("{}p", s.video[cur].height))
                        .width(70.0)
                        .show_ui(ui, |ui| {
                            for (i, f) in s.video.iter().enumerate() {
                                let label = if f.fps > 30 {
                                    format!("{}p{}", f.height, f.fps)
                                } else {
                                    format!("{}p", f.height)
                                };
                                ui.selectable_value(&mut sel, i, label);
                            }
                        });
                    if sel != cur {
                        self.watch.quality = sel;
                        let at = st.pos;
                        self.start_stream(at);
                    }
                }
            });
        });
    }

    fn set_fullscreen(&mut self, ctx: &egui::Context, on: bool) {
        self.fullscreen = on;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
    }

    fn watch_page(&mut self, ctx: &egui::Context) {
        let mut go = None;

        if self.fullscreen {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(Color32::BLACK))
                .show(ctx, |ui| {
                    let r = ui.available_size();
                    self.player_area(ui, ctx, r.x, r.y);
                });
            return;
        }

        egui::SidePanel::right("related")
            .resizable(false)
            .exact_width(380.0_f32.min(ctx.screen_rect().width() * 0.4))
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.label(RichText::new("Up next").strong().size(16.0));
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let items = self.watch.info.as_ref().map(|i| i.related.clone()).unwrap_or_default();
                    ui.add_space(6.0);
                    for v in items {
                        let w = ui.available_width() - 8.0;
                        if let Some(p) = self.video_card(ui, ctx, &v, w) {
                            go = Some(p);
                        }
                        ui.add_space(10.0);
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let w = ui.available_width() - 6.0;
                let h = (w * 9.0 / 16.0).min(ctx.screen_rect().height() * 0.72);
                let w = h * 16.0 / 9.0;
                self.player_area(ui, ctx, w, h);
                ui.set_max_width(w);
                ui.add_space(10.0);

                let title = self
                    .watch
                    .info
                    .as_ref()
                    .map(|i| i.title.clone())
                    .or_else(|| self.watch.streams.as_ref().map(|s| s.title.clone()))
                    .unwrap_or_default();
                ui.label(RichText::new(title).size(21.0).strong());
                ui.add_space(8.0);

                if let Some(info) = self.watch.info.clone() {
                    ui.horizontal(|ui| {
                        if let Some(t) = self.thumb(ctx, &info.channel_avatar) {
                            let r = ui.add(
                                egui::Image::new(egui::load::SizedTexture::new(t.id(), egui::vec2(40.0, 40.0)))
                                    .corner_radius(20.0)
                                    .sense(egui::Sense::click()),
                            );
                            if r.clicked() {
                                go = Some(Page::Channel(info.channel_id.clone()));
                            }
                        }
                        if ui
                            .add(egui::Label::new(RichText::new(&info.channel).strong().size(15.0)).sense(egui::Sense::click()))
                            .clicked()
                            && !info.channel_id.is_empty()
                        {
                            go = Some(Page::Channel(info.channel_id.clone()));
                        }
                        ui.add_space(8.0);
                        self.sub_button(ui, &info.channel_id, &info.channel, &info.channel_avatar);
                    });
                    ui.add_space(8.0);
                    egui::Frame::new()
                        .fill(Color32::from_white_alpha(14))
                        .corner_radius(10.0)
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(&info.views).strong());
                            let full = info.description.trim().to_string();
                            let desc: String = if self.watch.desc_open {
                                full.clone()
                            } else {
                                full.lines().take(3).collect::<Vec<_>>().join("\n")
                            };
                            ui.label(desc);
                            if full.lines().count() > 3
                                && ui.link(if self.watch.desc_open { "Show less" } else { "...more" }).clicked()
                            {
                                self.watch.desc_open = !self.watch.desc_open;
                            }
                        });
                    ui.add_space(16.0);
                    ui.label(RichText::new(format!("{} comments loaded", self.watch.comments.len())).strong().size(16.0));
                    ui.add_space(8.0);
                    let comments = self.watch.comments.clone();
                    for c in &comments {
                        ui.horizontal_top(|ui| {
                            if let Some(t) = self.thumb(ctx, &c.avatar) {
                                ui.add(
                                    egui::Image::new(egui::load::SizedTexture::new(t.id(), egui::vec2(36.0, 36.0)))
                                        .corner_radius(18.0),
                                );
                            } else {
                                ui.add_space(36.0);
                            }
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&c.author).strong().size(13.0));
                                    ui.label(RichText::new(&c.when).weak().size(12.0));
                                });
                                ui.label(&c.text);
                                let mut meta = String::new();
                                if !c.likes.is_empty() && c.likes != "0" {
                                    meta += &format!("👍 {}", c.likes);
                                }
                                if !c.replies.is_empty() && c.replies != "0" {
                                    meta += &format!("   💬 {}", c.replies);
                                }
                                if !meta.is_empty() {
                                    ui.label(RichText::new(meta).weak().size(12.0));
                                }
                            });
                        });
                        ui.add_space(10.0);
                    }
                    if self.watch.comments_loading {
                        ui.spinner();
                    } else if let Some(tok) = self.watch.comments_next.clone() {
                        if ui.button("Load more comments").clicked() {
                            self.load_comments(ctx, tok);
                        }
                    }
                } else if self.watch.error.is_none() {
                    ui.spinner();
                }
            });
        });

        if let Some(p) = go {
            self.navigate(ctx, p);
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if !matches!(self.page(), Page::Watch(_)) || ctx.wants_keyboard_input() {
            return;
        }
        let (space, left, right, up, down, f, esc, j, l, m) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::K),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::F),
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::J),
                i.key_pressed(egui::Key::L),
                i.key_pressed(egui::Key::M),
            )
        });
        let Some(p) = &self.player else { return };
        if space {
            p.toggle_pause();
        }
        if left {
            p.seek_rel(-5.0);
        }
        if right {
            p.seek_rel(5.0);
        }
        if j {
            p.seek_rel(-10.0);
        }
        if l {
            p.seek_rel(10.0);
        }
        if up || down {
            self.volume = (self.volume + if up { 5.0 } else { -5.0 }).clamp(0.0, 100.0);
            p.set_volume(self.volume as f64);
        }
        if m {
            self.volume = if self.volume > 0.0 { 0.0 } else { 100.0 };
            p.set_volume(self.volume as f64);
        }
        if f {
            self.set_fullscreen(ctx, !self.fullscreen);
        }
        if esc && self.fullscreen {
            self.set_fullscreen(ctx, false);
        }
    }
}

fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Add OS fonts as fallbacks so non-Latin titles (CJK, Tamil, Arabic, ...) render.
fn setup_fonts(ctx: &egui::Context) {
    let candidates: &[&str] = &[
        // macOS
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
        "/System/Library/Fonts/Supplemental/Tamil Sangam MN.ttc",
        "/System/Library/Fonts/Supplemental/Devanagari Sangam MN.ttc",
        "/System/Library/Fonts/Supplemental/Thonburi.ttc",
        // Windows
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\malgun.ttf",
        "C:\\Windows\\Fonts\\YuGothR.ttc",
        "C:\\Windows\\Fonts\\Nirmala.ttf",
        "C:\\Windows\\Fonts\\seguisym.ttf",
    ];
    let mut fonts = egui::FontDefinitions::default();
    for (i, path) in candidates.iter().enumerate() {
        if let Ok(bytes) = std::fs::read(path) {
            let name = format!("os-font-{i}");
            fonts
                .font_data
                .insert(name.clone(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push(name.clone());
            }
        }
    }
    ctx.set_fonts(fonts);
}

fn setup_style(ctx: &egui::Context) {
    setup_fonts(ctx);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(15, 15, 15);
    visuals.window_fill = Color32::from_rgb(24, 24, 24);
    visuals.extreme_bg_color = Color32::from_rgb(18, 18, 18);
    visuals.selection.bg_fill = RED;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, Color32::from_gray(70));
    visuals.extreme_bg_color = Color32::from_gray(30);
    visuals.hyperlink_color = Color32::from_rgb(98, 176, 255);
    ctx.set_visuals(visuals);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
    });
}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_progress();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump(ctx);
        self.shortcuts(ctx);

        if let Some(e) = self.player_err.clone() {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("RustTube can't start its video engine");
                ui.label(e);
                ui.label("Make sure libmpv is installed (macOS: `brew install mpv`; Windows: libmpv-2.dll next to the app).");
            });
            return;
        }

        let page = self.page();
        let watching = matches!(page, Page::Watch(_));
        if !(watching && self.fullscreen) {
            self.top_bar(ctx);
        }
        match page {
            Page::Watch(_) => self.watch_page(ctx),
            p => self.feed_page(ctx, p),
        }

        if std::env::var_os("RUSTTUBE_DEBUG").is_some() {
            let t = ctx.input(|i| i.time);
            if t - self.last_debug > 1.0 {
                self.last_debug = t;
                if let Some(p) = &self.player {
                    eprintln!("[debug] {:?} results={} err={:?}", p.state(), self.results.len(), self.watch.error);
                }
            }
        }
        if watching {
            let t = ctx.input(|i| i.time);
            if t - self.last_progress_save > 10.0 {
                self.last_progress_save = t;
                self.save_progress();
            }
        }
        if watching {
            // Keep the seek bar and clock moving.
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RustTube")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 420.0]),
        vsync: true,
        ..Default::default()
    };
    eframe::run_native("RustTube", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
