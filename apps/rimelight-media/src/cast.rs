//! Casting to a TV or speaker on the network (UPnP / DLNA MediaRenderer), without Windows' Cast panel. The devices are
//! found with SSDP, the open file is served over HTTP from this PC, and the device is told to play it (AVTransport
//! SOAP calls). Std only: no crates, no unsafe code.

use std::cell::RefCell;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use crate::app::{App, toast, with_app};
use crate::ui;

const SSDP_ADDR: &str = "239.255.255.250:1900";
const RENDERER: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";
const AVT: &str = "urn:schemas-upnp-org:service:AVTransport:1";

#[derive(Clone)]
struct Device {
    name: String,
    /// The AVTransport control URL (SOAP).
    control: String,
}

struct Target {
    name: String,
    control: String,
}

#[derive(Default)]
struct State {
    devices: Vec<Device>,
    target: Option<Target>,
    playing: bool,
}

thread_local! {
    // UI thread only: the last search and the cast in progress.
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// This PC's file server: one listener, serving whatever file is set now.
struct Server {
    port: u16,
    ip: String,
    file: Arc<Mutex<Option<PathBuf>>>,
}

static SERVER: OnceLock<Server> = OnceLock::new();

/// Starts a device search on a background thread; the list fills in when it ends.
pub fn refresh(app: &Rc<App>) {
    let cast = app.ui.global::<ui::Cast>();
    cast.set_searching(true);
    cast.set_searched(false);
    std::thread::spawn(|| {
        let devices = discover();
        let _ = slint::invoke_from_event_loop(move || {
            with_app(|a| {
                let names: Vec<SharedString> = devices
                    .iter()
                    .map(|d| SharedString::from(d.name.as_str()))
                    .collect();
                STATE.with(|s| s.borrow_mut().devices = devices);
                let cast = a.ui.global::<ui::Cast>();
                cast.set_devices(ModelRc::new(VecModel::from(names)));
                cast.set_searching(false);
                cast.set_searched(true);
            })
        });
    });
}

/// Casts the file that is open (the player's or the viewer's) to the device picked in the list.
pub fn pick(app: &Rc<App>, index: usize) {
    let Some(path) = current_file(app) else {
        toast(app, "Open a picture, video or song to cast it");
        return;
    };
    let Some(device) = STATE.with(|s| s.borrow().devices.get(index).cloned()) else {
        return;
    };
    let Some(server) = server() else {
        toast(app, "Could not start the file server for casting");
        return;
    };
    *server.file.lock().unwrap_or_else(|e| e.into_inner()) = Some(path.clone());
    let url = format!("http://{}:{}/{}", server.ip, server.port, url_name(&path));
    let mime = mime_of(&path);
    let meta = didl(&url, &file_name(&path), mime);
    let (name, control) = (device.name, device.control);
    std::thread::spawn(move || {
        let result = set_uri(&control, &url, &meta).and_then(|_| av_play(&control));
        let _ = slint::invoke_from_event_loop(move || {
            with_app(|a| match result {
                Ok(()) => {
                    STATE.with(|s| {
                        let mut s = s.borrow_mut();
                        s.target = Some(Target { name, control });
                        s.playing = true;
                    });
                    show(a);
                }
                Err(e) => toast(a, format!("Could not cast to {name}: {e}")),
            })
        });
    });
}

/// Pause, or play again, on the device.
pub fn toggle_play() {
    let Some((control, playing)) = STATE.with(|s| {
        let s = s.borrow();
        s.target.as_ref().map(|t| (t.control.clone(), s.playing))
    }) else {
        return;
    };
    std::thread::spawn(move || {
        let result = if playing {
            av_pause(&control)
        } else {
            av_play(&control)
        };
        let _ = slint::invoke_from_event_loop(move || {
            with_app(|a| match result {
                Ok(()) => {
                    STATE.with(|s| s.borrow_mut().playing = !playing);
                    show(a);
                }
                Err(e) => toast(
                    a,
                    format!("Could not {}: {e}", if playing { "pause" } else { "play" }),
                ),
            })
        });
    });
}

/// Stops the device and forgets the cast.
pub fn stop() {
    let Some(control) = STATE.with(|s| s.borrow().target.as_ref().map(|t| t.control.clone()))
    else {
        return;
    };
    std::thread::spawn(move || {
        let result = av_stop(&control);
        let _ = slint::invoke_from_event_loop(move || {
            with_app(|a| {
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    s.target = None;
                    s.playing = false;
                });
                show(a);
                if let Err(e) = result {
                    toast(a, format!("Could not stop: {e}"));
                }
            })
        });
    });
}

/// Shows the cast in progress (or none) in the popup.
fn show(app: &Rc<App>) {
    let (name, playing) = STATE.with(|s| {
        let s = s.borrow();
        (
            s.target
                .as_ref()
                .map(|t| t.name.clone())
                .unwrap_or_default(),
            s.playing,
        )
    });
    let cast = app.ui.global::<ui::Cast>();
    cast.set_casting_to(SharedString::from(name.as_str()));
    cast.set_playing(playing);
}

/// The file to cast: the picture when a picture is open, else the song or video.
fn current_file(app: &Rc<App>) -> Option<PathBuf> {
    let on_picture = app.ui.global::<ui::Shell>().get_page().as_str() == "image";
    let media = app.media.borrow().path.clone();
    let viewer = app.view.borrow().path.clone();
    let file = if on_picture {
        viewer.or(media)
    } else {
        media.or(viewer)
    };
    file.filter(|p| p.is_file())
}

fn server() -> Option<&'static Server> {
    if let Some(s) = SERVER.get() {
        return Some(s);
    }
    let listener = TcpListener::bind("0.0.0.0:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    let ip = lan_ip()?;
    let file = Arc::new(Mutex::new(None));
    let shared = Arc::clone(&file);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let shared = Arc::clone(&shared);
            std::thread::spawn(move || {
                let _ = serve(stream, &shared);
            });
        }
    });
    let _ = SERVER.set(Server { port, ip, file });
    SERVER.get()
}

/// This PC's address on the LAN. Connecting a UDP socket sends nothing; it only picks the route.
fn lan_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

/// Answers one HTTP request with the current file, with Range support for seeking on the device.
fn serve(mut stream: TcpStream, shared: &Mutex<Option<PathBuf>>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && head.len() < 8192 {
        if stream.read(&mut byte)? == 0 {
            break;
        }
        head.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&head).into_owned();
    let method = text.split_whitespace().next().unwrap_or("GET").to_string();
    let path = shared.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Some(path) = path else {
        return stream.write_all(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
    };
    let Ok(mut file) = std::fs::File::open(&path) else {
        return stream.write_all(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
    };
    let total = file.metadata()?.len();
    let (start, end, partial) = match header(&text, "range").and_then(|r| parse_range(&r, total)) {
        Some((s, e)) => (s, e, true),
        None => (0, total.saturating_sub(1), false),
    };
    let len = if total == 0 { 0 } else { end - start + 1 };
    let status = if partial {
        "206 Partial Content"
    } else {
        "200 OK"
    };
    let mut reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {}\r\nContent-Length: {len}\r\nAccept-Ranges: bytes\r\n",
        mime_of(&path)
    );
    if partial {
        reply.push_str(&format!("Content-Range: bytes {start}-{end}/{total}\r\n"));
    }
    reply.push_str("transferMode.dlna.org: Streaming\r\ncontentFeatures.dlna.org: *\r\nConnection: close\r\n\r\n");
    stream.write_all(reply.as_bytes())?;
    if method == "HEAD" || len == 0 {
        return Ok(());
    }
    file.seek(SeekFrom::Start(start))?;
    let mut left = len;
    let mut buf = vec![0u8; 64 * 1024];
    while left > 0 {
        let want = left.min(buf.len() as u64) as usize;
        let n = file.read(&mut buf[..want])?;
        if n == 0 {
            break;
        }
        stream.write_all(&buf[..n])?;
        left -= n as u64;
    }
    Ok(())
}

/// "bytes=a-b", "bytes=a-" or "bytes=-n" (the first range only) as an inclusive start and end.
fn parse_range(range: &str, total: u64) -> Option<(u64, u64)> {
    if total == 0 {
        return None;
    }
    let spec = range
        .trim()
        .strip_prefix("bytes=")?
        .split(',')
        .next()?
        .trim();
    let (a, b) = spec.split_once('-')?;
    let (start, end) = if a.is_empty() {
        let n: u64 = b.parse().ok()?;
        (total.saturating_sub(n), total - 1)
    } else {
        let s: u64 = a.parse().ok()?;
        let e = if b.is_empty() {
            total - 1
        } else {
            b.parse::<u64>().ok()?.min(total - 1)
        };
        (s, e)
    };
    (start <= end && start < total).then_some((start, end))
}

/// Finds the renderers on the network: sends an SSDP search and reads the answers for about 3 seconds.
fn discover() -> Vec<Device> {
    let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
        return Vec::new();
    };
    let _ = socket.set_read_timeout(Some(Duration::from_millis(250)));
    let search = format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {SSDP_ADDR}\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: {RENDERER}\r\n\r\n"
    );
    for _ in 0..3 {
        let _ = socket.send_to(search.as_bytes(), SSDP_ADDR);
    }
    let mut locations: Vec<String> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut buf = [0u8; 2048];
    while Instant::now() < deadline {
        let Ok((n, _)) = socket.recv_from(&mut buf) else {
            continue;
        };
        let answer = String::from_utf8_lossy(&buf[..n]);
        if let Some(location) = header(&answer, "location") {
            if !locations.contains(&location) {
                locations.push(location);
            }
        }
    }
    locations.iter().filter_map(|l| describe(l)).collect()
}

/// Reads a device description: its name and the AVTransport control URL.
fn describe(location: &str) -> Option<Device> {
    let reply = http("GET", location, &[], &[]).ok()?;
    if reply.status != 200 {
        return None;
    }
    let xml = String::from_utf8_lossy(&reply.body).into_owned();
    let name = tag(&xml, "friendlyName").unwrap_or_else(|| origin(location));
    let base = tag(&xml, "URLBase").unwrap_or_else(|| location.to_string());
    let service = xml.find("service:AVTransport:")?;
    let control = tag(&xml[service..], "controlURL")?;
    Some(Device {
        name,
        control: resolve(&base, &control),
    })
}

/// The text between <name> and </name>.
fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = start + xml[start..].find(&format!("</{name}>"))?;
    Some(xml[start..end].trim().to_string())
}

/// The value of a header (the name is matched without case).
fn header(message: &str, name: &str) -> Option<String> {
    message.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
    })
}

/// "http://host:port" of a URL.
fn origin(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("http", url));
    let host = rest.split('/').next().unwrap_or(rest);
    format!("{scheme}://{host}")
}

/// A control URL from a device description: absolute, or relative to URLBase (or the description's own address).
fn resolve(base: &str, rel: &str) -> String {
    if rel.contains("://") {
        return rel.to_string();
    }
    let o = origin(base);
    if rel.starts_with('/') {
        return format!("{o}{rel}");
    }
    let path = base.get(o.len()..).unwrap_or("");
    let dir = path.rfind('/').map_or("", |i| &path[..i]);
    format!("{o}{dir}/{rel}")
}

struct Reply {
    status: u16,
    body: Vec<u8>,
}

/// A small HTTP/1.1 client: one request on a new connection (Connection: close).
fn http(method: &str, url: &str, headers: &[(&str, String)], body: &[u8]) -> Result<Reply, String> {
    let rest = url.strip_prefix("http://").ok_or("not an http address")?;
    let (host, path) = match rest.split_once('/') {
        Some((h, p)) => (h, format!("/{p}")),
        None => (rest, "/".to_string()),
    };
    let addr = host
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no address")?;
    let mut stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|e| e.to_string())?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(8)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(8)));
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        request.push_str(&format!("{k}: {v}\r\n"));
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;
    stream.write_all(body).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    // A timeout after the answer arrived still leaves the answer in `raw`.
    let _ = stream.read_to_end(&mut raw);
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("no answer")?;
    let status_line = String::from_utf8_lossy(&raw[..split]);
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("bad answer")?;
    Ok(Reply {
        status,
        body: raw[split + 4..].to_vec(),
    })
}

/// One AVTransport action with its arguments (XML), as a SOAP call.
fn soap(control: &str, action: &str, args: &str) -> Result<(), String> {
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action} xmlns:u="{AVT}"><InstanceID>0</InstanceID>{args}</u:{action}></s:Body></s:Envelope>"#
    );
    let headers = [
        ("Content-Type", "text/xml; charset=\"utf-8\"".to_string()),
        ("SOAPACTION", format!("\"{AVT}#{action}\"")),
    ];
    let reply = http("POST", control, &headers, body.as_bytes())?;
    if reply.status == 200 {
        Ok(())
    } else {
        Err(format!("{action} failed (HTTP {})", reply.status))
    }
}

fn set_uri(control: &str, url: &str, meta: &str) -> Result<(), String> {
    let args = format!(
        "<CurrentURI>{}</CurrentURI><CurrentURIMetaData>{}</CurrentURIMetaData>",
        esc(url),
        esc(meta)
    );
    soap(control, "SetAVTransportURI", &args)
}

fn av_play(control: &str) -> Result<(), String> {
    soap(control, "Play", "<Speed>1</Speed>")
}

fn av_pause(control: &str) -> Result<(), String> {
    soap(control, "Pause", "")
}

fn av_stop(control: &str) -> Result<(), String> {
    soap(control, "Stop", "")
}

/// Jumps to a time (seconds from the current position). Kept for remote control; no button uses it yet.
#[allow(dead_code)]
fn av_seek(control: &str, secs: u64) -> Result<(), String> {
    let target = format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60);
    soap(
        control,
        "Seek",
        &format!("<Unit>REL_TIME</Unit><Target>{target}</Target>"),
    )
}

/// The DIDL-Lite description that goes with the URL (title and protocol info).
fn didl(url: &str, title: &str, mime: &str) -> String {
    let class = if mime.starts_with("audio/") {
        "object.item.audioItem"
    } else if mime.starts_with("image/") {
        "object.item.imageItem"
    } else {
        "object.item.videoItem"
    };
    format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="0" parentID="-1" restricted="1"><dc:title>{}</dc:title><upnp:class>{class}</upnp:class><res protocolInfo="http-get:*:{mime}:*">{}</res></item></DIDL-Lite>"#,
        esc(title),
        esc(url)
    )
}

/// XML text escaping.
fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Media")
        .to_string()
}

/// The file name in the URL, percent-encoded (the extension stays, so the device knows the type).
fn url_name(path: &Path) -> String {
    file_name(path)
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn mime_of(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "m4a" => "audio/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "mpg" | "mpeg" => "video/mpeg",
        "ts" => "video/mp2t",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "aac" => "audio/aac",
        "wma" => "audio/x-ms-wma",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
}
