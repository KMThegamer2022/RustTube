//! Loopback page host for YouTube's official embedded player.
//!
//! The embed API needs an http(s) origin, so the app serves a tiny page on
//! 127.0.0.1 that loads the IFrame Player API and reports playback status back
//! to the app through the web view's IPC channel.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::OnceLock;

static PORT: OnceLock<u16> = OnceLock::new();

/// Start the page server (once) and return its port.
pub fn start() -> u16 {
    *PORT.get_or_init(|| {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                std::thread::spawn(move || {
                    let _ = handle(conn);
                });
            }
        });
        port
    })
}

pub fn url(video_id: &str, start: f64) -> String {
    format!("http://127.0.0.1:{}/p?v={}&t={}", start_port(), video_id, start as u64)
}

fn start_port() -> u16 {
    start()
}

fn handle(mut conn: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(conn.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
    // Drain headers.
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h.trim().is_empty() {
            break;
        }
    }
    let (route, query) = path.split_once('?').unwrap_or((path.as_str(), ""));
    if route != "/p" {
        conn.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
        return Ok(());
    }
    let mut vid = String::new();
    let mut t: u64 = 0;
    for kv in query.split('&') {
        match kv.split_once('=') {
            Some(("v", v)) => vid = v.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').take(20).collect(),
            Some(("t", v)) => t = v.parse().unwrap_or(0),
            _ => {}
        }
    }
    let body = PAGE.replace("__VID__", &vid).replace("__START__", &t.to_string());
    write!(
        conn,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8">
<meta name="referrer" content="strict-origin-when-cross-origin">
<style>html,body{margin:0;height:100%;background:#000;overflow:hidden}#p{width:100%;height:100%}</style>
</head><body><div id="p"></div>
<script src="https://www.youtube.com/iframe_api"></script>
<script>
var player, err = 0;
function send(){
  try {
    var o = {t: player.getCurrentTime ? player.getCurrentTime() : 0,
             d: player.getDuration ? player.getDuration() : 0,
             s: player.getPlayerState ? player.getPlayerState() : -1, e: err};
    window.ipc.postMessage(JSON.stringify(o));
  } catch (x) {}
}
function onYouTubeIframeAPIReady(){
  player = new YT.Player('p', {
    width: '100%', height: '100%', videoId: '__VID__',
    playerVars: {autoplay: 1, start: __START__, rel: 0, playsinline: 1, enablejsapi: 1,
                 origin: location.origin, fs: 0},
    events: {
      onReady: function(e){ e.target.playVideo(); send(); },
      onStateChange: send,
      onError: function(e){ err = e.data; send(); }
    }
  });
  setInterval(send, 1000);
}
</script></body></html>
"#;
