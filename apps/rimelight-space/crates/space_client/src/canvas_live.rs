//! Live room session for a canvas (CV.4, decision 0045). Gets a ticket over REST, connects the WebSocket, reports what the room sends, resends ops the room has not acknowledged after a reconnect, and reconnects with backoff. Native only for now: on wasm32 `LiveSession::connect` reports `LiveEvent::Ended { reason: "unsupported" }` (live editing in the browser comes with the web slice).

use std::sync::Arc;
use std::time::Duration;

use space_proto::canvases::{ClientOp, Hello, MediaControl, Presence, ServerMessage};

#[cfg(not(target_arch = "wasm32"))]
use futures_util::{Sink, SinkExt, StreamExt};
#[cfg(not(target_arch = "wasm32"))]
use space_proto::canvases::ClientMessage;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicI64, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::mpsc::UnboundedReceiver;
#[cfg(not(target_arch = "wasm32"))]
use tokio_tungstenite::tungstenite::Message;

use crate::api::ApiClient;
use crate::error::ClientError;

/// How often the session sends a ping while connected.
pub const PING_EVERY: Duration = Duration::from_secs(25);

/// Reconnect delay for attempt n (0-based): 500 ms doubling, at most 15 s.
pub fn retry_delay(attempt: u32) -> Duration {
    let ms = 500u64.saturating_mul(1u64 << attempt.min(16));
    Duration::from_millis(ms.min(15_000))
}

/// The WebSocket URL for a ticket path: https -> wss, http -> ws, base_url with any trailing '/' removed, then path.
pub fn live_url(base_url: &str, path: &str) -> Result<String, ClientError> {
    let base = base_url.trim_end_matches('/');
    let (scheme, rest) = if let Some(rest) = base.strip_prefix("https://") {
        ("wss://", rest)
    } else if let Some(rest) = base.strip_prefix("http://") {
        ("ws://", rest)
    } else {
        return Err(ClientError::Protocol(format!(
            "not an http base URL: {base_url}"
        )));
    };
    Ok(format!("{scheme}{rest}{path}"))
}

/// Local wall-clock time in epoch milliseconds.
#[cfg(not(target_arch = "wasm32"))]
fn local_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

/// Estimated server clock minus local clock, from one ping: the server stamped its pong at `server_now`, and the
/// round trip ran from `sent_ms` to `received_ms` (local times), so the server was at the middle of it.
#[cfg(not(target_arch = "wasm32"))]
pub fn clock_offset_ms(server_now: i64, sent_ms: i64, received_ms: i64) -> i64 {
    server_now - (sent_ms + received_ms) / 2
}

/// Ops sent but not yet acknowledged, in send order, resent after a reconnect.
#[derive(Debug, Default)]
pub struct PendingOps {
    ops: Vec<ClientOp>,
}

impl PendingOps {
    pub fn push(&mut self, op: ClientOp) {
        self.ops.push(op);
    }

    /// Removes and returns the op with this id (acked or rejected).
    pub fn settle(&mut self, op_id: &str) -> Option<ClientOp> {
        let index = self.ops.iter().position(|op| op.op_id == op_id)?;
        Some(self.ops.remove(index))
    }

    pub fn pending(&self) -> &[ClientOp] {
        &self.ops
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

/// Op ids for one session: "c1", "c2", ...
#[derive(Debug, Default)]
pub struct OpIds {
    next: u64,
}

impl OpIds {
    pub fn next_id(&mut self) -> String {
        self.next += 1;
        format!("c{}", self.next)
    }
}

/// What the session reports to the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum LiveEvent {
    /// The room's hello: the full state. Sent on the first connect and after every reconnect.
    Connected(Hello),
    /// Any other message from the room (Ack, Applied, Reject, Presence, Join, Leave, Canvas, Error).
    Message(ServerMessage),
    /// The socket dropped; the session reconnects after `retry_in`.
    Disconnected { reason: String, retry_in: Duration },
    /// The session stopped for good: "closed" (by `close`), "deleted", "unauthorized", "not_found", "unsupported".
    Ended { reason: String },
}

/// The callback that receives events. Called on the session's own thread: hand the event to the UI thread from there.
#[cfg(not(target_arch = "wasm32"))]
pub type EventSink = Arc<dyn Fn(LiveEvent) + Send + Sync>;
#[cfg(target_arch = "wasm32")]
pub type EventSink = std::rc::Rc<dyn Fn(LiveEvent)>;

/// A running live session. Dropping it closes the session.
#[cfg(not(target_arch = "wasm32"))]
pub struct LiveSession {
    tx: tokio::sync::mpsc::UnboundedSender<Command>,
    /// Server clock minus local clock in milliseconds, from the latest ping; 0 until the first pong.
    offset: Arc<AtomicI64>,
}

#[cfg(target_arch = "wasm32")]
pub struct LiveSession {}

#[cfg(not(target_arch = "wasm32"))]
enum Command {
    Op(ClientOp),
    Presence(Presence),
    Media(MediaControl),
    Close,
}

impl LiveSession {
    /// Starts the session. Native: on its own thread with a current-thread tokio runtime.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn connect(
        api: Arc<ApiClient>,
        token: String,
        canvas_id: String,
        events: EventSink,
    ) -> LiveSession {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let report = events.clone();
        let offset = Arc::new(AtomicI64::new(0));
        let clock = offset.clone();
        let spawned = std::thread::Builder::new()
            .name("canvas-live".into())
            .spawn(move || {
                match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => {
                        runtime.block_on(run(api, token, canvas_id, rx, events, clock));
                    }
                    Err(e) => events(LiveEvent::Ended {
                        reason: format!("runtime: {e}"),
                    }),
                }
            });
        if let Err(e) = spawned {
            report(LiveEvent::Ended {
                reason: format!("thread: {e}"),
            });
        }
        LiveSession { tx, offset }
    }

    /// Starts the session. Browser: live editing is not built yet, so this only reports that.
    #[cfg(target_arch = "wasm32")]
    pub fn connect(
        api: Arc<ApiClient>,
        token: String,
        canvas_id: String,
        events: EventSink,
    ) -> LiveSession {
        let _ = (api, token, canvas_id);
        events(LiveEvent::Ended {
            reason: "unsupported".into(),
        });
        LiveSession {}
    }

    /// Queues an op (its op_id must be set, see OpIds); kept until the room acks or rejects it, resent after reconnects.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn send_op(&self, op: ClientOp) {
        let _ = self.tx.send(Command::Op(op));
    }

    #[cfg(target_arch = "wasm32")]
    pub fn send_op(&self, _op: ClientOp) {}

    /// Sends presence now if connected; dropped otherwise (the next one replaces it).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn send_presence(&self, presence: Presence) {
        let _ = self.tx.send(Command::Presence(presence));
    }

    #[cfg(target_arch = "wasm32")]
    pub fn send_presence(&self, _presence: Presence) {}

    /// Sends a shared video control now if connected; dropped otherwise (the room has no replay of it).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn send_media(&self, control: MediaControl) {
        let _ = self.tx.send(Command::Media(control));
    }

    #[cfg(target_arch = "wasm32")]
    pub fn send_media(&self, _control: MediaControl) {}

    /// The server's clock in epoch milliseconds: local time plus the offset measured from the latest ping.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn server_now_ms(&self) -> i64 {
        local_ms() + self.offset.load(Ordering::Relaxed)
    }

    /// Browser: there is no clock offset yet, so this reports 0 (the browser has no SystemTime).
    #[cfg(target_arch = "wasm32")]
    pub fn server_now_ms(&self) -> i64 {
        0
    }

    /// Closes the socket and ends the session (LiveEvent::Ended { reason: "closed" }).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn close(&self) {
        let _ = self.tx.send(Command::Close);
    }

    #[cfg(target_arch = "wasm32")]
    pub fn close(&self) {}
}

impl Drop for LiveSession {
    fn drop(&mut self) {
        self.close()
    }
}

/// The session's loop on its own thread: ticket, connect, pump messages, reconnect with backoff.
#[cfg(not(target_arch = "wasm32"))]
async fn run(
    api: Arc<ApiClient>,
    token: String,
    canvas_id: String,
    mut rx: UnboundedReceiver<Command>,
    events: EventSink,
    offset: Arc<AtomicI64>,
) {
    let mut pending = PendingOps::default();
    let mut attempt: u32 = 0;
    'outer: loop {
        // 1. Ticket.
        let ticket = match api.canvas_live_ticket(&token, &canvas_id).await {
            Ok(ticket) => ticket,
            Err(ClientError::Unauthorized) => {
                events(LiveEvent::Ended {
                    reason: "unauthorized".into(),
                });
                return;
            }
            Err(ClientError::Api { status: 404, .. }) => {
                events(LiveEvent::Ended {
                    reason: "not_found".into(),
                });
                return;
            }
            Err(e) => {
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
                events(LiveEvent::Ended {
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

        // 3. Pump messages until the socket drops.
        let (mut sink, mut stream) = ws.split();
        let mut ping = tokio::time::interval(PING_EVERY);
        // The first tick of an interval fires at once; the first ping is one period after connecting.
        ping.tick().await;
        let mut connected = false;
        let mut skip_wait = false;
        // Local time of the ping whose pong is awaited; the first ping goes out now so the clock offset is known early.
        let mut ping_sent_ms = Some(local_ms());
        let _ = send_json(&mut sink, &ClientMessage::Ping).await;
        'session: loop {
            tokio::select! {
                msg = stream.next() => match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ServerMessage>(text.as_str()) {
                            Ok(ServerMessage::Hello(hello)) => {
                                attempt = 0;
                                connected = true;
                                events(LiveEvent::Connected(hello));
                                for op in pending.pending() {
                                    if !send_json(&mut sink, &ClientMessage::Op(op.clone())).await {
                                        break 'session;
                                    }
                                }
                            }
                            Ok(ServerMessage::Closed { reason }) => {
                                if reason == "deleted" {
                                    events(LiveEvent::Ended { reason: "deleted".into() });
                                    return;
                                }
                                // The room closed this connection on purpose: reconnect now.
                                skip_wait = true;
                                break 'session;
                            }
                            Ok(ServerMessage::Pong { now: Some(server_now) }) => {
                                if let Some(sent) = ping_sent_ms.take() {
                                    let offset_ms = clock_offset_ms(server_now, sent, local_ms());
                                    offset.store(offset_ms, Ordering::Relaxed);
                                }
                            }
                            Ok(ServerMessage::Pong { now: None } | ServerMessage::Unknown) => {}
                            Ok(msg) => {
                                if let ServerMessage::Ack { op_id, .. }
                                | ServerMessage::Reject { op_id, .. } = &msg
                                {
                                    pending.settle(op_id);
                                }
                                events(LiveEvent::Message(msg));
                            }
                            Err(e) => events(LiveEvent::Message(ServerMessage::Error {
                                message: format!("unreadable message: {e}"),
                            })),
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        if frame.map(|f| u16::from(f.code)) == Some(4000) {
                            events(LiveEvent::Ended { reason: "deleted".into() });
                            return;
                        }
                        break 'session;
                    }
                    // Ping, pong and binary frames need no action here.
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => break 'session,
                },
                cmd = rx.recv() => match cmd {
                    Some(Command::Op(op)) => {
                        let wire = connected.then(|| ClientMessage::Op(op.clone()));
                        pending.push(op);
                        let sent = match wire {
                            Some(msg) => send_json(&mut sink, &msg).await,
                            None => true,
                        };
                        if !sent {
                            break 'session;
                        }
                    }
                    Some(Command::Presence(presence)) => {
                        if connected {
                            // A failed send means the socket is dead; the read side notices and reconnects.
                            let _ = send_json(&mut sink, &ClientMessage::Presence(presence)).await;
                        }
                    }
                    Some(Command::Media(control)) => {
                        if connected {
                            let _ = send_json(&mut sink, &ClientMessage::Media(control)).await;
                        }
                    }
                    Some(Command::Close) | None => {
                        let _ = sink.send(Message::Close(None)).await;
                        events(LiveEvent::Ended { reason: "closed".into() });
                        return;
                    }
                },
                _ = ping.tick() => {
                    ping_sent_ms = Some(local_ms());
                    if !send_json(&mut sink, &ClientMessage::Ping).await {
                        break 'session;
                    }
                }
            }
        }

        // 4. The socket dropped: back off, then reconnect.
        if !skip_wait
            && !wait_or_close(
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

/// Reports the drop, then waits out the backoff for this attempt. Ops sent meanwhile are queued. Returns false when the
/// session was closed during the wait (it has then reported `Ended`).
#[cfg(not(target_arch = "wasm32"))]
async fn wait_or_close(
    rx: &mut UnboundedReceiver<Command>,
    pending: &mut PendingOps,
    events: &EventSink,
    attempt: &mut u32,
    reason: String,
) -> bool {
    let delay = retry_delay(*attempt);
    *attempt = attempt.saturating_add(1);
    events(LiveEvent::Disconnected {
        reason,
        retry_in: delay,
    });
    let deadline = tokio::time::Instant::now() + delay;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => return true,
            cmd = rx.recv() => match cmd {
                Some(Command::Op(op)) => pending.push(op),
                Some(Command::Presence(_) | Command::Media(_)) => {}
                Some(Command::Close) | None => {
                    events(LiveEvent::Ended { reason: "closed".into() });
                    return false;
                }
            },
        }
    }
}

/// Sends one client message as JSON text. Returns false when the socket refused it.
#[cfg(not(target_arch = "wasm32"))]
async fn send_json<S>(sink: &mut S, msg: &ClientMessage) -> bool
where
    S: Sink<Message> + Unpin,
{
    match serde_json::to_string(msg) {
        Ok(text) => sink.send(Message::Text(text.into())).await.is_ok(),
        Err(_) => true,
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::sync::{Arc, Mutex};

    use futures_util::{SinkExt, StreamExt};
    use space_proto::canvases::{OpAction, ServerMessage};
    use tokio_tungstenite::tungstenite::Message;

    use super::*;
    use crate::testing::{ScriptedTransport, json_response};

    const ITEM_ID: &str = "11111111-1111-4111-8111-111111111111";

    const HELLO: &str = r##"{"t":"hello","you":{"conn_id":"a","user_id":"u","name":"N","color":"#000","access":"edit"},"seq":0,"canvas":{"title":"T","archived":false},"items":[],"peers":[]}"##;

    fn op(op_id: &str) -> ClientOp {
        ClientOp {
            op_id: op_id.to_string(),
            item_id: ITEM_ID.to_string(),
            base: 0,
            action: OpAction::Create,
            kind: Some("Text".to_string()),
            z: Some("a0".to_string()),
            props: None,
        }
    }

    #[test]
    fn retry_delay_doubles_then_caps() {
        assert_eq!(retry_delay(0), Duration::from_millis(500));
        assert_eq!(retry_delay(1), Duration::from_secs(1));
        assert_eq!(retry_delay(5), Duration::from_secs(15));
        assert_eq!(retry_delay(20), Duration::from_secs(15));
    }

    #[test]
    fn live_url_maps_schemes() {
        assert_eq!(
            live_url("https://rimelight.com", "/v1/canvases/c/live?ticket=t").unwrap(),
            "wss://rimelight.com/v1/canvases/c/live?ticket=t"
        );
        assert_eq!(
            live_url("http://127.0.0.1:9/", "/v1/canvases/c/live?ticket=t").unwrap(),
            "ws://127.0.0.1:9/v1/canvases/c/live?ticket=t"
        );
        assert!(matches!(
            live_url("ftp://x", "/p"),
            Err(ClientError::Protocol(_))
        ));
    }

    #[test]
    fn pending_ops_keep_send_order_and_settle_by_id() {
        let mut pending = PendingOps::default();
        assert!(pending.is_empty());
        pending.push(op("c1"));
        pending.push(op("c2"));
        pending.push(op("c3"));
        assert_eq!(pending.len(), 3);
        let ids: Vec<&str> = pending.pending().iter().map(|o| o.op_id.as_str()).collect();
        assert_eq!(ids, ["c1", "c2", "c3"]);

        assert_eq!(
            pending.settle("c2").map(|o| o.op_id),
            Some("c2".to_string())
        );
        assert_eq!(pending.settle("c2"), None);
        let ids: Vec<&str> = pending.pending().iter().map(|o| o.op_id.as_str()).collect();
        assert_eq!(ids, ["c1", "c3"]);
    }

    #[test]
    fn clock_offset_takes_the_middle_of_the_round_trip() {
        // Sent at local 400, received at 600, server said 1000 at the middle (500): the server runs 500 ms ahead.
        assert_eq!(clock_offset_ms(1000, 400, 600), 500);
        // A server 300 ms behind the local clock.
        assert_eq!(clock_offset_ms(1000, 1290, 1310), -300);
    }

    #[test]
    fn op_ids_count_up_from_c1() {
        let mut ids = OpIds::default();
        assert_eq!(ids.next_id(), "c1");
        assert_eq!(ids.next_id(), "c2");
    }

    /// Server: sends a hello, acks the first op it gets, then closes the socket and stops (one connection only).
    fn spawn_room(port_tx: std::sync::mpsc::Sender<u16>) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("server runtime");
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("bind");
                let port = listener.local_addr().expect("addr").port();
                port_tx.send(port).expect("send port");
                let (stream, _) = listener.accept().await.expect("accept");
                let mut ws = tokio_tungstenite::accept_async(stream)
                    .await
                    .expect("handshake");
                ws.send(Message::Text(HELLO.into())).await.expect("hello");
                while let Some(msg) = ws.next().await {
                    let Ok(Message::Text(text)) = msg else {
                        break;
                    };
                    let value: serde_json::Value =
                        serde_json::from_str(text.as_str()).expect("client json");
                    if value["t"] == "op" {
                        let reply = serde_json::json!({
                            "t": "ack",
                            "op_id": value["op_id"].clone(),
                            "seq": 1,
                            "item": {
                                "id": value["item_id"].clone(),
                                "kind": "Text",
                                "z": "a0",
                                "version": 1,
                                "props": {},
                                "deleted": false,
                                "created_by": "u",
                                "updated_by": "u",
                                "updated_at": 1
                            }
                        });
                        ws.send(Message::Text(reply.to_string().into()))
                            .await
                            .expect("ack");
                        break;
                    }
                }
                ws.send(Message::Close(None)).await.ok();
            });
        })
    }

    #[test]
    fn session_acks_an_op_then_ends_unauthorized_after_the_drop() {
        let (port_tx, port_rx) = std::sync::mpsc::channel::<u16>();
        let room = spawn_room(port_tx);
        let port = port_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("room port");

        // Ticket 1 succeeds; the reconnect's ticket call is refused, so the session ends.
        let transport = Arc::new(ScriptedTransport::new(vec![
            json_response(
                200,
                r#"{"ticket":"t1","expires_in_s":60,"path":"/live?ticket=t1"}"#,
            ),
            json_response(401, r#"{"error":"unauthorized","message":"x"}"#),
        ]));
        let api = Arc::new(ApiClient::with_transport(
            format!("http://127.0.0.1:{port}"),
            transport,
        ));

        let log: Arc<Mutex<Vec<LiveEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let (wake_tx, wake_rx) = std::sync::mpsc::channel::<LiveEvent>();
        let sink_log = log.clone();
        let events: EventSink = Arc::new(move |event: LiveEvent| {
            crate::lock(&sink_log).push(event.clone());
            let _ = wake_tx.send(event);
        });

        let session = LiveSession::connect(api, "tok".into(), "canvas".into(), events);
        let next = |what: &str| {
            wake_rx
                .recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|e| panic!("waiting for {what}: {e}"))
        };

        assert!(matches!(next("hello"), LiveEvent::Connected(_)));
        session.send_op(op("c1"));
        let ack = next("ack");
        assert!(
            matches!(&ack, LiveEvent::Message(ServerMessage::Ack { op_id, .. }) if op_id == "c1"),
            "expected ack for c1, got {ack:?}"
        );
        assert!(matches!(next("drop"), LiveEvent::Disconnected { .. }));
        let ended = next("ended");
        assert!(
            matches!(&ended, LiveEvent::Ended { reason } if reason == "unauthorized"),
            "expected unauthorized, got {ended:?}"
        );

        let kinds: Vec<&str> = crate::lock(&log)
            .iter()
            .map(|e| match e {
                LiveEvent::Connected(_) => "connected",
                LiveEvent::Message(ServerMessage::Ack { .. }) => "ack",
                LiveEvent::Message(_) => "message",
                LiveEvent::Disconnected { .. } => "disconnected",
                LiveEvent::Ended { .. } => "ended",
            })
            .collect();
        assert_eq!(kinds, ["connected", "ack", "disconnected", "ended"]);

        drop(session);
        room.join().expect("room thread");
    }
}

/// Makes aws-lc-rs the process-wide rustls crypto provider, once. rustls picks a provider by itself only when exactly one of
/// its backends is compiled in; another crate in the app may enable `ring` too, and then the WebSocket connect would panic.
/// An already installed provider is kept.
#[cfg(not(target_arch = "wasm32"))]
fn install_crypto_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}
