//! Live editing session for a content document (Space CMS.5, decision 0049): gets a ticket over REST for the page's main
//! room or one author draft, connects the binary WebSocket, reports the room's frames and events, sends local Loro
//! updates and presence, buffers updates made while the socket is down, and reconnects with the canvas backoff.
//!
//! Wire format (website `apps/rimelight.com/src/space/content/doc-room-logic.ts`): every frame is binary. The first
//! byte is the type, and `rimelight_content::sync::{encode, decode}` read and write it: 0x00 snapshot, 0x01 update,
//! 0x02 presence, 0x03 event (JSON with a "type": "comments", "closed", "published", "readonly" or "error").
//! A `closed` event ends the session; the room has stopped. The main room of a page is read-only for the socket
//! (reviewers follow the published line); its updates are refused with a `readonly` event.
//!
//! Native only for now: on wasm32 `ContentLiveSession::connect` reports `ContentLiveEvent::Ended { reason: "unsupported" }`
//! (the browser editor comes with the web slice), as `canvas_live` does.

use std::sync::Arc;
use std::time::Duration;

use rimelight_content::sync::Frame;

#[cfg(not(target_arch = "wasm32"))]
use futures_util::{Sink, SinkExt, StreamExt};
#[cfg(not(target_arch = "wasm32"))]
use rimelight_content::sync::{decode, encode};
#[cfg(not(target_arch = "wasm32"))]
use serde_json::Value;
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::mpsc::UnboundedReceiver;
#[cfg(not(target_arch = "wasm32"))]
use tokio_tungstenite::tungstenite::Message;

use crate::api::ApiClient;
#[cfg(not(target_arch = "wasm32"))]
use crate::canvas_live::{PING_EVERY, live_url, retry_delay};
#[cfg(not(target_arch = "wasm32"))]
use crate::error::ClientError;

/// Which document a session opens: the page's main room, or one author's draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentRoom {
    /// A page id (`POST /v1/content/pages/:id/live-ticket`). The socket is read-only.
    Page(String),
    /// A draft id (`POST /v1/content/drafts/:id/live-ticket`). The socket may send updates.
    Draft(String),
}

/// A room event that does not end the session (the `closed` event does).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomEvent {
    /// Someone commented on the page or the draft: reload the comment list.
    Comments,
    /// The draft was published.
    Published,
    /// The socket tried to send an update it may not make (a read-only socket).
    Readonly,
    /// The room refused a frame; the message says why.
    Error(String),
    /// An event type this client does not know yet.
    Other(String),
}

/// What the session reports to the UI.
#[derive(Debug)]
pub enum ContentLiveEvent {
    /// The socket is open. The room sends a snapshot frame next, and again after every reconnect.
    Connected,
    /// A snapshot, update or presence frame from the room. Hand it to `SyncSession::receive`.
    Frame(Frame),
    /// A room event (see [`RoomEvent`]).
    Event(RoomEvent),
    /// The socket dropped; the session reconnects after `retry_in`.
    Disconnected { reason: String, retry_in: Duration },
    /// The session stopped for good: "closed" (by `close`, or the room's `closed` event with its reason), "unauthorized",
    /// "forbidden", "not_found", "draft_closed" (the ticket was refused because the draft is no longer open),
    /// "unsupported", or "protocol".
    Ended { reason: String },
}

/// The callback that receives events. Called on the session's own thread: hand the event to the UI thread from there.
#[cfg(not(target_arch = "wasm32"))]
pub type ContentEventSink = Arc<dyn Fn(ContentLiveEvent) + Send + Sync>;
#[cfg(target_arch = "wasm32")]
pub type ContentEventSink = std::rc::Rc<dyn Fn(ContentLiveEvent)>;

/// Wraps a callback as the sink type of this target (Arc natively, Rc on wasm, where the session runs on the UI thread).
pub fn content_sink(f: impl Fn(ContentLiveEvent) + Send + Sync + 'static) -> ContentEventSink {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Arc::new(f)
    }
    #[cfg(target_arch = "wasm32")]
    {
        std::rc::Rc::new(f)
    }
}

/// Reads a room event frame's JSON. `closed` returns its reason, which ends the session.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    Room(RoomEvent),
    Closed(String),
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_event(value: &Value) -> Parsed {
    let kind = value["type"].as_str().unwrap_or("");
    match kind {
        "comments" => Parsed::Room(RoomEvent::Comments),
        "published" => Parsed::Room(RoomEvent::Published),
        "readonly" => Parsed::Room(RoomEvent::Readonly),
        "closed" => Parsed::Closed(value["reason"].as_str().unwrap_or("closed").to_string()),
        "error" => Parsed::Room(RoomEvent::Error(
            value["message"].as_str().unwrap_or("").to_string(),
        )),
        other => Parsed::Room(RoomEvent::Other(other.to_string())),
    }
}

/// Updates made while the socket is down, in order. Loro updates are idempotent, so resending them after a reconnect
/// is safe; presence is never queued (the next one replaces it).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PendingUpdates {
    updates: Vec<Vec<u8>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl PendingUpdates {
    pub fn push(&mut self, bytes: Vec<u8>) {
        self.updates.push(bytes);
    }

    pub fn len(&self) -> usize {
        self.updates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.updates.is_empty()
    }

    /// The queued updates in send order, without removing them.
    pub fn pending(&self) -> &[Vec<u8>] {
        &self.updates
    }

    /// Drops the first `count` updates once they have been sent.
    pub fn sent(&mut self, count: usize) {
        self.updates.drain(..count.min(self.updates.len()));
    }
}

/// The ticket error a session ends on, or None when the ticket should be retried after a backoff.
#[cfg(not(target_arch = "wasm32"))]
fn ticket_end_reason(error: &ClientError) -> Option<&'static str> {
    match error {
        ClientError::Unauthorized => Some("unauthorized"),
        ClientError::Api { status: 403, .. } => Some("forbidden"),
        ClientError::Api { status: 404, .. } => Some("not_found"),
        ClientError::Api { status: 409, .. } => Some("draft_closed"),
        _ => None,
    }
}

/// A running content live session. Dropping it closes the session.
#[cfg(not(target_arch = "wasm32"))]
pub struct ContentLiveSession {
    tx: tokio::sync::mpsc::UnboundedSender<Command>,
}

#[cfg(target_arch = "wasm32")]
pub struct ContentLiveSession {}

#[cfg(not(target_arch = "wasm32"))]
enum Command {
    Update(Vec<u8>),
    Presence(Vec<u8>),
    Close,
}

impl ContentLiveSession {
    /// Starts the session. Native: on its own thread with a current-thread tokio runtime.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn connect(
        api: Arc<ApiClient>,
        token: String,
        room: ContentRoom,
        events: ContentEventSink,
    ) -> ContentLiveSession {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let report = events.clone();
        let spawned = std::thread::Builder::new()
            .name("content-live".into())
            .spawn(move || {
                match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime.block_on(run(api, token, room, rx, events)),
                    Err(e) => events(ContentLiveEvent::Ended {
                        reason: format!("runtime: {e}"),
                    }),
                }
            });
        if let Err(e) = spawned {
            report(ContentLiveEvent::Ended {
                reason: format!("thread: {e}"),
            });
        }
        ContentLiveSession { tx }
    }

    /// Starts the session. Browser: the live editor is not built yet, so this only reports that.
    #[cfg(target_arch = "wasm32")]
    pub fn connect(
        api: Arc<ApiClient>,
        token: String,
        room: ContentRoom,
        events: ContentEventSink,
    ) -> ContentLiveSession {
        let _ = (api, token, room);
        events(ContentLiveEvent::Ended {
            reason: "unsupported".into(),
        });
        ContentLiveSession {}
    }

    /// Sends a local Loro update (bytes from `SyncSession::outgoing`). Queued while the socket is down, sent on reconnect.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn send_update(&self, bytes: Vec<u8>) {
        let _ = self.tx.send(Command::Update(bytes));
    }

    #[cfg(target_arch = "wasm32")]
    pub fn send_update(&self, _bytes: Vec<u8>) {}

    /// Sends presence now if connected; dropped otherwise (the next one replaces it).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn send_presence(&self, bytes: Vec<u8>) {
        let _ = self.tx.send(Command::Presence(bytes));
    }

    #[cfg(target_arch = "wasm32")]
    pub fn send_presence(&self, _bytes: Vec<u8>) {}

    /// Closes the socket and ends the session (`ContentLiveEvent::Ended { reason: "closed" }`).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn close(&self) {
        let _ = self.tx.send(Command::Close);
    }

    #[cfg(target_arch = "wasm32")]
    pub fn close(&self) {}
}

impl Drop for ContentLiveSession {
    fn drop(&mut self) {
        self.close()
    }
}

/// The session's loop on its own thread: ticket, connect, pump frames, reconnect with backoff.
#[cfg(not(target_arch = "wasm32"))]
async fn run(
    api: Arc<ApiClient>,
    token: String,
    room: ContentRoom,
    mut rx: UnboundedReceiver<Command>,
    events: ContentEventSink,
) {
    let mut pending = PendingUpdates::default();
    let mut attempt: u32 = 0;
    'outer: loop {
        // 1. Ticket.
        let ticket = match &room {
            ContentRoom::Page(id) => api.content_page_live_ticket(&token, id).await,
            ContentRoom::Draft(id) => api.content_draft_live_ticket(&token, id).await,
        };
        let ticket = match ticket {
            Ok(ticket) => ticket,
            Err(e) => {
                if let Some(reason) = ticket_end_reason(&e) {
                    events(ContentLiveEvent::Ended {
                        reason: reason.into(),
                    });
                    return;
                }
                if !wait_or_close(&mut rx, &mut pending, &events, &mut attempt, e.to_string()).await
                {
                    return;
                }
                continue 'outer;
            }
        };

        // 2. Connect.
        let url = match live_url(api.base_url(), &ticket.path) {
            Ok(url) => url,
            Err(_) => {
                events(ContentLiveEvent::Ended {
                    reason: "protocol".into(),
                });
                return;
            }
        };
        install_crypto_provider();
        let ws = match tokio_tungstenite::connect_async(url.as_str()).await {
            Ok((ws, _)) => ws,
            Err(e) => {
                if !wait_or_close(&mut rx, &mut pending, &events, &mut attempt, e.to_string()).await
                {
                    return;
                }
                continue 'outer;
            }
        };
        attempt = 0;
        events(ContentLiveEvent::Connected);

        // 3. Pump frames until the socket drops.
        let (mut sink, mut stream) = ws.split();
        let mut ping = tokio::time::interval(PING_EVERY);
        // The first tick of an interval fires at once; the first ping goes out one period after connecting.
        ping.tick().await;

        // Updates made while the socket was down go out first, in order.
        let total = pending.len();
        let mut sent = 0;
        for bytes in pending.pending() {
            if !send_frame(&mut sink, &Frame::Update(bytes.clone())).await {
                break;
            }
            sent += 1;
        }
        pending.sent(sent);
        if sent < total {
            // The socket refused a queued update: reconnect and try again.
            if !wait_or_close(
                &mut rx,
                &mut pending,
                &events,
                &mut attempt,
                "connection lost".into(),
            )
            .await
            {
                return;
            }
            continue 'outer;
        }

        'session: loop {
            tokio::select! {
                msg = stream.next() => match msg {
                    Some(Ok(Message::Binary(data))) => match decode(&data) {
                        Ok(Frame::Event(value)) => match parse_event(&value) {
                            Parsed::Closed(reason) => {
                                // The room has stopped (published, discarded, or deleted): no reconnect.
                                events(ContentLiveEvent::Ended { reason });
                                return;
                            }
                            Parsed::Room(event) => events(ContentLiveEvent::Event(event)),
                        },
                        Ok(frame) => events(ContentLiveEvent::Frame(frame)),
                        Err(e) => events(ContentLiveEvent::Event(RoomEvent::Error(
                            format!("unreadable frame: {e}"),
                        ))),
                    },
                    // The room speaks binary frames only; text frames and protocol pings need no action here.
                    Some(Ok(Message::Text(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break 'session,
                },
                cmd = rx.recv() => match cmd {
                    Some(Command::Update(bytes)) => {
                        if !send_frame(&mut sink, &Frame::Update(bytes.clone())).await {
                            pending.push(bytes);
                            break 'session;
                        }
                    }
                    Some(Command::Presence(bytes)) => {
                        // A failed send means the socket is dead; the read side notices and reconnects.
                        let _ = send_frame(&mut sink, &Frame::Presence(bytes)).await;
                    }
                    Some(Command::Close) | None => {
                        let _ = sink.send(Message::Close(None)).await;
                        events(ContentLiveEvent::Ended { reason: "closed".into() });
                        return;
                    }
                },
                _ = ping.tick() => {
                    if sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                        break 'session;
                    }
                }
            }
        }

        // 4. The socket dropped: back off, then reconnect.
        if !wait_or_close(
            &mut rx,
            &mut pending,
            &events,
            &mut attempt,
            "connection lost".to_string(),
        )
        .await
        {
            return;
        }
    }
}

/// Reports the drop, then waits out the backoff for this attempt. Updates sent meanwhile are queued; presence is dropped.
/// Returns false when the session was closed during the wait (it has then reported `Ended`).
#[cfg(not(target_arch = "wasm32"))]
async fn wait_or_close(
    rx: &mut UnboundedReceiver<Command>,
    pending: &mut PendingUpdates,
    events: &ContentEventSink,
    attempt: &mut u32,
    reason: String,
) -> bool {
    let delay = retry_delay(*attempt);
    *attempt = attempt.saturating_add(1);
    events(ContentLiveEvent::Disconnected {
        reason,
        retry_in: delay,
    });
    let deadline = tokio::time::Instant::now() + delay;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => return true,
            cmd = rx.recv() => match cmd {
                Some(Command::Update(bytes)) => pending.push(bytes),
                Some(Command::Presence(_)) => {}
                Some(Command::Close) | None => {
                    events(ContentLiveEvent::Ended { reason: "closed".into() });
                    return false;
                }
            },
        }
    }
}

/// Encodes one frame and sends it as a binary message. Returns false when the socket refused it.
#[cfg(not(target_arch = "wasm32"))]
async fn send_frame<S>(sink: &mut S, frame: &Frame) -> bool
where
    S: Sink<Message> + Unpin,
{
    sink.send(Message::Binary(encode(frame).into()))
        .await
        .is_ok()
}

/// Makes aws-lc-rs the process-wide rustls crypto provider, once (the same reason as in `canvas_live`).
#[cfg(not(target_arch = "wasm32"))]
fn install_crypto_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn events_map_to_room_events_and_closed_ends() {
        assert_eq!(
            parse_event(&json!({"type": "comments"})),
            Parsed::Room(RoomEvent::Comments)
        );
        assert_eq!(
            parse_event(&json!({"type": "published"})),
            Parsed::Room(RoomEvent::Published)
        );
        assert_eq!(
            parse_event(&json!({"type": "readonly"})),
            Parsed::Room(RoomEvent::Readonly)
        );
        assert_eq!(
            parse_event(&json!({"type": "error", "message": "too big"})),
            Parsed::Room(RoomEvent::Error("too big".into()))
        );
        assert_eq!(
            parse_event(&json!({"type": "closed", "reason": "discarded"})),
            Parsed::Closed("discarded".into())
        );
        assert_eq!(
            parse_event(&json!({"type": "closed"})),
            Parsed::Closed("closed".into())
        );
        assert_eq!(
            parse_event(&json!({"type": "later"})),
            Parsed::Room(RoomEvent::Other("later".into()))
        );
    }

    #[test]
    fn ticket_refusals_end_and_others_retry() {
        assert_eq!(
            ticket_end_reason(&ClientError::Unauthorized),
            Some("unauthorized")
        );
        assert_eq!(
            ticket_end_reason(&ClientError::Api {
                status: 409,
                error: "draft_closed".into(),
                message: "x".into(),
            }),
            Some("draft_closed")
        );
        assert_eq!(
            ticket_end_reason(&ClientError::Network("down".into())),
            None
        );
    }

    #[test]
    fn pending_updates_keep_order_and_drop_what_was_sent() {
        let mut pending = PendingUpdates::default();
        assert!(pending.is_empty());
        pending.push(vec![1]);
        pending.push(vec![2]);
        pending.push(vec![3]);
        assert_eq!(pending.pending(), &[vec![1], vec![2], vec![3]]);
        pending.sent(2);
        assert_eq!(pending.pending(), &[vec![3]]);
        pending.sent(10);
        assert!(pending.is_empty());
    }
}
