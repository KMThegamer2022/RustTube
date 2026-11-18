//! libmpv playback, rendered straight into the egui window through OpenGL.

use std::ffi::{c_void, CStr};
use std::sync::{Arc, Mutex};

use eframe::egui;
use eframe::egui_glow;
use eframe::glow::{self, HasContext};
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};
use libmpv2::Mpv;

type GetProc = *const (dyn Fn(&CStr) -> *const c_void + 'static);

thread_local! {
    // Only set while the render context is being created on this thread.
    static GET_PROC: std::cell::Cell<Option<GetProc>> = const { std::cell::Cell::new(None) };
}

fn get_proc_address(_: &(), name: &str) -> *mut c_void {
    let Ok(c) = std::ffi::CString::new(name) else {
        return std::ptr::null_mut();
    };
    GET_PROC.with(|g| match g.get() {
        // SAFETY: the pointer is valid for the duration of Player::new.
        Some(f) => unsafe { (*f)(&c) as *mut c_void },
        None => std::ptr::null_mut(),
    })
}

struct Fbo {
    fbo: glow::Framebuffer,
    tex: glow::Texture,
    w: i32,
    h: i32,
}

struct GlState {
    ctx: RenderContext<'static>,
    fbo: Option<Fbo>,
}

// The render context is only ever touched on the GL thread (inside the paint
// callback), always behind the mutex.
unsafe impl Send for GlState {}

#[derive(Clone, Copy, Default, Debug)]
pub struct PlayState {
    pub pos: f64,
    pub duration: f64,
    pub paused: bool,
    pub buffering: bool,
    pub ended: bool,
    pub volume: f64,
    pub has_file: bool,
}

pub struct Player {
    mpv: &'static Mpv,
    gl: Arc<Mutex<GlState>>,
    pub has_file: bool,
}

impl Player {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Player, String> {
        let get = cc
            .get_proc_address
            .ok_or("no OpenGL proc address available")?;
        // SAFETY: lifetime erased; cleared again before `get` can dangle.
        let erased: GetProc = unsafe { std::mem::transmute(get as *const (dyn Fn(&CStr) -> *const c_void + '_)) };
        GET_PROC.with(|g| g.set(Some(erased)));

        let mpv = Mpv::with_initializer(|init| {
            init.set_property("vo", "libmpv")?;
            init.set_property("hwdec", "auto-safe")?;
            init.set_property("user-agent", crate::yt::VR_UA)?;
            init.set_property("keep-open", "yes")?;
            if std::env::var_os("RUSTTUBE_DEBUG").is_some() {
                init.set_property("terminal", "yes")?;
                init.set_property("msg-level", "all=v")?;
            } else {
                init.set_property("terminal", "no")?;
            }
            init.set_property("cache", "yes")?;
            init.set_property("demuxer-max-bytes", "150MiB")?;
            init.set_property("demuxer-readahead-secs", 60i64)?;
            init.set_property("network-timeout", 15i64)?;
            init.set_property("audio-client-name", "RustTube")?;
            init.set_property("osc", "no")?;
            init.set_property("ytdl", "no")?;
            init.set_property("input-default-bindings", "no")?;
            Ok(())
        })
        .map_err(|e| format!("could not start libmpv: {e}"))?;
        let mpv: &'static Mpv = Box::leak(Box::new(mpv));

        let mut ctx = mpv
            .create_render_context(vec![
                RenderParam::ApiType(RenderParamApiType::OpenGl),
                RenderParam::InitParams(OpenGLInitParams {
                    get_proc_address,
                    ctx: (),
                }),
            ])
            .map_err(|e| format!("could not create mpv render context: {e}"))?;
        GET_PROC.with(|g| g.set(None));
        let egui_ctx = cc.egui_ctx.clone();
        ctx.set_update_callback(move || egui_ctx.request_repaint());

        Ok(Player {
            mpv,
            gl: Arc::new(Mutex::new(GlState { ctx, fbo: None })),
            has_file: false,
        })
    }

    pub fn load(&mut self, video_url: &str, audio_url: &str, start: f64) {
        let _ = self.mpv.set_property("audio-file", audio_url);
        let _ = self.mpv.set_property("start", format!("{start:.2}").as_str());
        let _ = self.mpv.set_property("pause", false);
        if let Err(e) = self.mpv.command("loadfile", &[video_url, "replace"]) {
            eprintln!("loadfile failed: {e}");
        }
        self.has_file = true;
    }

    pub fn stop(&mut self) {
        let _ = self.mpv.command("stop", &[]);
        self.has_file = false;
    }

    pub fn toggle_pause(&self) {
        let p: bool = self.mpv.get_property("pause").unwrap_or(false);
        if self.state().ended {
            let _ = self.mpv.command("seek", &["0", "absolute"]);
            let _ = self.mpv.set_property("pause", false);
        } else {
            let _ = self.mpv.set_property("pause", !p);
        }
    }

    pub fn seek_rel(&self, secs: f64) {
        let _ = self.mpv.command("seek", &[&format!("{secs}"), "relative"]);
    }

    pub fn seek_abs(&self, secs: f64) {
        let _ = self.mpv.command("seek", &[&format!("{secs:.2}"), "absolute"]);
    }

    pub fn set_volume(&self, v: f64) {
        let _ = self.mpv.set_property("volume", v.clamp(0.0, 130.0));
    }

    pub fn set_speed(&self, s: f64) {
        let _ = self.mpv.set_property("speed", s);
    }

    pub fn state(&self) -> PlayState {
        PlayState {
            pos: self.mpv.get_property("time-pos").unwrap_or(0.0),
            duration: self.mpv.get_property("duration").unwrap_or(0.0),
            paused: self.mpv.get_property("pause").unwrap_or(false),
            buffering: self.mpv.get_property("paused-for-cache").unwrap_or(false),
            ended: self.mpv.get_property("eof-reached").unwrap_or(false),
            volume: self.mpv.get_property("volume").unwrap_or(100.0),
            has_file: self.has_file,
        }
    }

    /// Paint the current video frame into `rect`.
    pub fn paint(&self, ui: &egui::Ui, rect: egui::Rect) {
        let gl_state = self.gl.clone();
        let cb = egui_glow::CallbackFn::new(move |info, painter| {
            let vp = info.viewport_in_pixels();
            let (w, h) = (vp.width_px, vp.height_px);
            if w <= 0 || h <= 0 {
                return;
            }
            let gl = painter.gl();
            let Ok(mut st) = gl_state.lock() else { return };
            unsafe {
                let prev = gl.get_parameter_i32(glow::FRAMEBUFFER_BINDING);
                let prev_fb = std::num::NonZeroU32::new(prev as u32).map(glow::NativeFramebuffer);

                let need_new = st.fbo.as_ref().map_or(true, |f| f.w != w || f.h != h);
                if need_new {
                    if let Some(old) = st.fbo.take() {
                        gl.delete_framebuffer(old.fbo);
                        gl.delete_texture(old.tex);
                    }
                    let tex = gl.create_texture().unwrap();
                    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                    gl.tex_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        glow::RGBA8 as i32,
                        w,
                        h,
                        0,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(None),
                    );
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
                    let fbo = gl.create_framebuffer().unwrap();
                    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
                    gl.framebuffer_texture_2d(
                        glow::FRAMEBUFFER,
                        glow::COLOR_ATTACHMENT0,
                        glow::TEXTURE_2D,
                        Some(tex),
                        0,
                    );
                    st.fbo = Some(Fbo { fbo, tex, w, h });
                }
                let (fbo, fw, fh) = {
                    let f = st.fbo.as_ref().unwrap();
                    (f.fbo, f.w, f.h)
                };

                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
                gl.disable(glow::SCISSOR_TEST);
                let _ = st.ctx.render::<()>(fbo.0.get() as i32, fw, fh, true);

                gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(fbo));
                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, prev_fb);
                gl.blit_framebuffer(
                    0,
                    0,
                    fw,
                    fh,
                    vp.left_px,
                    vp.from_bottom_px,
                    vp.left_px + fw,
                    vp.from_bottom_px + fh,
                    glow::COLOR_BUFFER_BIT,
                    glow::LINEAR,
                );
                gl.bind_framebuffer(glow::FRAMEBUFFER, prev_fb);
                gl.enable(glow::SCISSOR_TEST);
            }
        });
        ui.painter().add(egui::PaintCallback {
            rect,
            callback: Arc::new(cb),
        });
    }
}
