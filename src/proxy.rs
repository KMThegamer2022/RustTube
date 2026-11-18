//! Tiny loopback HTTP proxy.
//!
//! The video CDN only accepts bounded `Range` requests, while mpv asks for
//! open-ended ones (`bytes=0-`). The proxy accepts mpv's request on localhost
//! and fetches the data upstream in bounded chunks, streaming it back.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

const CHUNK: u64 = 4 * 1024 * 1024;

struct Entry {
    url: String,
    len: u64,
}

static ENTRIES: OnceLock<Mutex<HashMap<u64, Entry>>> = OnceLock::new();
static PORT: OnceLock<u16> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(1);

fn entries() -> &'static Mutex<HashMap<u64, Entry>> {
    ENTRIES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Start the listener (once) and return a loopback URL serving `url`.
pub fn register(url: &str, len: u64) -> String {
    let port = *PORT.get_or_init(|| {
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
    });
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut map = entries().lock().unwrap_or_else(|e| e.into_inner());
    if map.len() > 64 {
        map.clear();
    }
    map.insert(id, Entry { url: url.to_string(), len });
    format!("http://127.0.0.1:{port}/{id}")
}

fn handle(mut conn: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(conn.try_clone()?);
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or("").to_string();
        let path = parts.next().unwrap_or("").to_string();

        let mut range: Option<(u64, Option<u64>)> = None;
        loop {
            let mut h = String::new();
            if reader.read_line(&mut h)? == 0 {
                return Ok(());
            }
            let h = h.trim();
            if h.is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':') {
                if k.eq_ignore_ascii_case("range") {
                    if let Some(spec) = v.trim().strip_prefix("bytes=") {
                        if let Some((a, b)) = spec.split_once('-') {
                            if let Ok(a) = a.trim().parse::<u64>() {
                                range = Some((a, b.trim().parse::<u64>().ok()));
                            }
                        }
                    }
                }
            }
        }

        let id: u64 = path.trim_start_matches('/').parse().unwrap_or(0);
        let (url, len) = {
            let map = entries().lock().unwrap_or_else(|e| e.into_inner());
            match map.get(&id) {
                Some(e) => (e.url.clone(), e.len),
                None => {
                    conn.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
                    return Ok(());
                }
            }
        };

        let (start, end) = match range {
            Some((a, b)) => (a, b.map_or(len.saturating_sub(1), |b| b.min(len.saturating_sub(1)))),
            None => (0, len.saturating_sub(1)),
        };
        if start >= len {
            conn.write_all(
                format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\nContent-Length: 0\r\n\r\n")
                    .as_bytes(),
            )?;
            continue;
        }
        let body_len = end - start + 1;
        let status = if range.is_some() { "206 Partial Content" } else { "200 OK" };
        let mut head = format!(
            "HTTP/1.1 {status}\r\nAccept-Ranges: bytes\r\nContent-Type: application/octet-stream\r\nContent-Length: {body_len}\r\n"
        );
        if range.is_some() {
            head += &format!("Content-Range: bytes {start}-{end}/{len}\r\n");
        }
        head += "\r\n";
        conn.write_all(head.as_bytes())?;
        if method == "HEAD" {
            continue;
        }

        let mut pos = start;
        while pos <= end {
            let chunk_end = (pos + CHUNK - 1).min(end);
            let resp = ureq::get(&url)
                .header("User-Agent", crate::yt::VR_UA)
                .header("Range", &format!("bytes={pos}-{chunk_end}"))
                .call();
            let Ok(resp) = resp else {
                return Ok(()); // drop the connection; mpv will retry
            };
            let mut body = resp.into_body().into_reader();
            let mut buf = [0u8; 64 * 1024];
            let mut got = 0u64;
            loop {
                let n = body.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                conn.write_all(&buf[..n])?;
                got += n as u64;
            }
            if got == 0 {
                return Ok(());
            }
            pos += got;
        }
    }
}
