//! One running copy. The first copy listens on a loopback port (written with a secret to %APPDATA%\Rimelight Media\
//! instance.txt); a later start (a double-clicked file, Win+Shift+S through ms-screenclip:, the Start menu) sends its
//! request there and exits, so there is one window, one tray icon and one set of global keys.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// What a start asks the running copy to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Bring the window up.
    Show,
    /// Open a file.
    Open(String),
    /// Start a snip: "region", "window", "screen", "free", "color" or "text".
    Snip(String),
    /// A start-with-Windows launch while a copy already runs: nothing to do.
    Nothing,
}

impl Request {
    fn encode(&self) -> String {
        match self {
            Request::Show => "show".into(),
            Request::Open(p) => format!("open\t{p}"),
            Request::Snip(m) => format!("snip\t{m}"),
            Request::Nothing => "nothing".into(),
        }
    }

    fn decode(line: &str) -> Option<Request> {
        let (kind, rest) = line.split_once('\t').unwrap_or((line, ""));
        Some(match kind {
            "show" => Request::Show,
            "open" if !rest.is_empty() => Request::Open(rest.to_string()),
            "snip" if !rest.is_empty() => Request::Snip(rest.to_string()),
            "nothing" => Request::Nothing,
            _ => return None,
        })
    }
}

fn lock_file() -> std::path::PathBuf {
    crate::prefs::data_dir().join("instance.txt")
}

fn secret() -> String {
    use std::hash::{BuildHasher as _, Hasher as _};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u32(std::process::id());
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    format!("{:016x}{:08x}", h.finish(), std::process::id())
}

/// Sends `request` to a running copy. True when one took it (this process should exit).
pub fn forward(request: &Request) -> bool {
    let Ok(text) = std::fs::read_to_string(lock_file()) else {
        return false;
    };
    let mut parts = text.split_whitespace();
    let (Some(port), Some(token)) = (
        parts.next().and_then(|p| p.parse::<u16>().ok()),
        parts.next(),
    ) else {
        return false;
    };
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(400)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    if writeln!(stream, "{token}\t{}", request.encode()).is_err() {
        return false;
    }
    // The running copy answers "ok" once it has the request.
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer).is_ok() && answer.trim() == "ok"
}

/// Becomes the running copy: listens for requests from later starts and hands each to `handle` on the UI thread.
pub fn serve(handle: fn(Request)) {
    let Ok(listener) = TcpListener::bind(("127.0.0.1", 0)) else {
        return;
    };
    let Ok(addr) = listener.local_addr() else {
        return;
    };
    let token = secret();
    let file = lock_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::write(&file, format!("{} {token}", addr.port())).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("rimelight-media-instance".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut line = String::new();
                let mut reader = BufReader::new(&stream);
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let line = line.trim_end_matches(['\r', '\n']);
                let Some((given, rest)) = line.split_once('\t') else {
                    continue;
                };
                if given != token {
                    continue;
                }
                if let Some(request) = Request::decode(rest) {
                    let _ = (&stream).write_all(b"ok\n");
                    let _ = slint::invoke_from_event_loop(move || handle(request));
                }
            }
        })
        .ok();
}

/// Removes the lock file when this copy exits (only if it is still ours).
pub fn release() {
    let _ = std::fs::remove_file(lock_file());
}

/// The snip mode an ms-screenclip: link asks for (Win+Shift+S, or the Print Screen key once the Snipping Tool is gone).
pub fn screenclip_mode(uri: &str) -> String {
    let lower = uri.to_ascii_lowercase();
    if lower.contains("clippingmode=window") {
        "window"
    } else if lower.contains("clippingmode=freeform") {
        "free"
    } else if lower.contains("clippingmode=fullscreen") {
        "screen"
    } else {
        "region"
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip() {
        for r in [
            Request::Show,
            Request::Open(r"C:\a b\c.png".into()),
            Request::Snip("free".into()),
            Request::Nothing,
        ] {
            assert_eq!(Request::decode(&r.encode()), Some(r));
        }
        assert_eq!(
            screenclip_mode("ms-screenclip:?source=HotKey&clippingMode=Window"),
            "window"
        );
        assert_eq!(screenclip_mode("ms-screenclip:"), "region");
    }
}
