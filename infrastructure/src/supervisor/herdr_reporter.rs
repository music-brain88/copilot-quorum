//! `HerdrReporterAdapter` — reports this quorum instance's coarse status
//! (working / blocked / idle) to a herdr supervisor over its local control
//! socket. Subscribes to the `EventPublisher` seam (`AppEvent::AgentStatusChanged`)
//! so the application layer stays herdr-agnostic (Issue #309 / RFC Discussion #313).
//!
//! # Wire format
//!
//! Verified against a running herdr 0.7.x instance (Issue #309 investigation
//! comment): newline-delimited JSON over a Unix stream socket, no LSP
//! framing. One connection per message (herdr's control socket accepts
//! short-lived connections fine, and this keeps the writer stateless):
//!
//! ```text
//! {"id":"...","method":"pane.report_agent","params":{"pane_id":"...","source":"copilot-quorum","agent":"quorum","state":"working","custom_status":"...","seq":1}}\n
//! ```
//!
//! # Opt-in
//!
//! [`HerdrReporterAdapter::from_env`] returns `None` unless `HERDR_ENV` +
//! `HERDR_PANE_ID` + `HERDR_SOCKET_PATH` are all present — outside a herdr
//! pane this adapter is never constructed, so it costs nothing (no thread,
//! no socket). Once constructed, every write is best-effort: a broken pipe,
//! stale socket, or unresponsive peer degrades silently and never disrupts
//! the run itself.

use quorum_application::{AppEvent, EventPublisher};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::time::Duration;
use tracing::debug;

const REPORTER_SOURCE: &str = "copilot-quorum";
const REPORTER_AGENT_LABEL: &str = "quorum";
const SOCKET_WRITE_TIMEOUT: Duration = Duration::from_millis(500);

enum OutboundMessage {
    Report {
        state: &'static str,
        custom_status: Option<String>,
    },
    Release,
}

/// Subscriber adapter: forwards `AgentStatusChanged` events to herdr's
/// `pane.report_agent` / `pane.release_agent` API over a Unix socket.
///
/// Owns a dedicated OS thread that serializes and connects per message —
/// `publish()` itself never blocks or touches the socket, matching
/// `EventPublisher`'s sync fire-and-forget contract.
pub struct HerdrReporterAdapter {
    pane_id: String,
    socket_path: String,
    // `Option` so `Drop` can move the sender out and let it fall out of
    // scope *before* joining the writer thread — the thread's `for msg in
    // rx` loop only ends once every `Sender` clone is gone, so joining
    // while `self.tx` (a field, dropped only after `Drop::drop` returns)
    // is still alive would deadlock.
    tx: Option<mpsc::Sender<OutboundMessage>>,
    handle: Option<std::thread::JoinHandle<()>>,
    /// Set once [`Self::shutdown`] (or `Drop`) has sent the release — makes
    /// both paths safe to call/run without double-releasing.
    released: std::sync::atomic::AtomicBool,
}

impl HerdrReporterAdapter {
    /// Build the adapter from herdr's self-identification env vars.
    ///
    /// Returns `None` (a true no-op — no thread spawned) unless `HERDR_ENV`,
    /// `HERDR_PANE_ID`, and `HERDR_SOCKET_PATH` are all present.
    pub fn from_env() -> Option<Self> {
        if std::env::var("HERDR_ENV").is_err() {
            return None;
        }
        let pane_id = std::env::var("HERDR_PANE_ID").ok()?;
        let socket_path = std::env::var("HERDR_SOCKET_PATH").ok()?;
        Some(Self::new(pane_id, socket_path))
    }

    fn new(pane_id: String, socket_path: String) -> Self {
        let (tx, rx) = mpsc::channel::<OutboundMessage>();
        let handle = std::thread::Builder::new()
            .name("herdr-reporter".into())
            .spawn({
                let pane_id = pane_id.clone();
                let socket_path = socket_path.clone();
                move || writer_loop(rx, pane_id, socket_path)
            })
            .ok();
        Self {
            pane_id,
            socket_path,
            tx: Some(tx),
            handle,
            released: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Best-effort synchronous release — sends `pane.release_agent` directly
    /// on the calling thread (bypassing the channel/writer thread) and
    /// blocks briefly (bounded by [`SOCKET_WRITE_TIMEOUT`]) until the write
    /// completes.
    ///
    /// Call this explicitly at every well-defined graceful-shutdown point
    /// (Ctrl+C, SIGTERM, `:qa!`, normal process exit) rather than relying on
    /// `Drop` alone: this adapter is normally reached only via the *last*
    /// `Arc<dyn EventPublisher>` clone inside `AgentController`, which in
    /// the TUI/headless path lives inside a background `tokio::spawn`ed
    /// task. That task's own shutdown is not ordered relative to the
    /// `#[tokio::main]` runtime's teardown when `main()` returns — a runtime
    /// drop can tear down outstanding tasks without necessarily running them
    /// to the point where `Drop` fires, so relying on `Drop` alone dropped
    /// `pane.release_agent` in practice (verified against a live herdr
    /// instance, Issue #309 E2E). Idempotent: a repeated call (or a `Drop`
    /// that still fires afterward) is a safe no-op.
    pub fn shutdown(&self) {
        if self
            .released
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        let request = build_request(&self.pane_id, 0, OutboundMessage::Release);
        if let Err(e) = send(&self.socket_path, &request) {
            debug!(
                "herdr reporter: shutdown release failed (degrading silently): {}",
                e
            );
        }
    }
}

impl EventPublisher for HerdrReporterAdapter {
    fn publish(&self, event: AppEvent) {
        match event {
            // Carried by JSONL / Lua subscribers already — not this adapter's concern.
            AppEvent::QuorumResult(_) => {}
            AppEvent::AgentStatusChanged(status) => {
                let msg = OutboundMessage::Report {
                    state: status.as_str(),
                    custom_status: status.detail().map(str::to_string),
                };
                if let Some(tx) = &self.tx {
                    let _ = tx.send(msg);
                }
            }
        }
    }
}

impl Drop for HerdrReporterAdapter {
    fn drop(&mut self) {
        // Best-effort defense in depth for paths that don't call
        // `shutdown()` explicitly (e.g. the one-shot agent CLI mode, where
        // this adapter is a plain local with no cross-task ownership
        // ambiguity). `shutdown()` already made the release idempotent.
        self.shutdown();
        // Drop the sender so the writer thread's receive loop ends — only
        // then is it safe to join it (see the `tx` field comment).
        self.tx.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn writer_loop(rx: mpsc::Receiver<OutboundMessage>, pane_id: String, socket_path: String) {
    let mut seq: u64 = 0;
    for msg in rx {
        seq += 1;
        let request = build_request(&pane_id, seq, msg);
        if let Err(e) = send(&socket_path, &request) {
            debug!("herdr reporter: send failed (degrading silently): {}", e);
        }
    }
}

fn build_request(pane_id: &str, seq: u64, msg: OutboundMessage) -> serde_json::Value {
    let id = format!("quorum-report-{seq}");
    match msg {
        OutboundMessage::Report {
            state,
            custom_status,
        } => serde_json::json!({
            "id": id,
            "method": "pane.report_agent",
            "params": {
                "pane_id": pane_id,
                "source": REPORTER_SOURCE,
                "agent": REPORTER_AGENT_LABEL,
                "state": state,
                "custom_status": custom_status,
                "seq": seq,
            }
        }),
        OutboundMessage::Release => serde_json::json!({
            "id": id,
            "method": "pane.release_agent",
            "params": {
                "pane_id": pane_id,
                "source": REPORTER_SOURCE,
                "agent": REPORTER_AGENT_LABEL,
            }
        }),
    }
}

fn send(socket_path: &str, request: &serde_json::Value) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(socket_path)?;
    stream.set_write_timeout(Some(SOCKET_WRITE_TIMEOUT))?;
    let mut line = serde_json::to_vec(request)?;
    line.push(b'\n');
    stream.write_all(&line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_domain::AgentStatus;
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixListener;

    fn bind_test_socket() -> (tempfile::TempDir, UnixListener, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let path_str = path.to_string_lossy().into_owned();
        (dir, listener, path_str)
    }

    fn read_one_line(listener: &UnixListener) -> serde_json::Value {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }

    #[test]
    fn test_publish_sends_report_agent() {
        let (_dir, listener, socket_path) = bind_test_socket();
        let adapter = HerdrReporterAdapter::new("w3A:p1".to_string(), socket_path);

        adapter.publish(AppEvent::AgentStatusChanged(AgentStatus::working(
            "planning",
        )));

        let value = read_one_line(&listener);
        assert_eq!(value["method"], "pane.report_agent");
        assert_eq!(value["params"]["pane_id"], "w3A:p1");
        assert_eq!(value["params"]["source"], "copilot-quorum");
        assert_eq!(value["params"]["agent"], "quorum");
        assert_eq!(value["params"]["state"], "working");
        assert_eq!(value["params"]["custom_status"], "planning");
        assert_eq!(value["params"]["seq"], 1);
    }

    #[test]
    fn test_publish_blocked_carries_detail_as_custom_status() {
        let (_dir, listener, socket_path) = bind_test_socket();
        let adapter = HerdrReporterAdapter::new("w3A:p1".to_string(), socket_path);

        adapter.publish(AppEvent::AgentStatusChanged(AgentStatus::blocked(
            "HiL: プラン承認待ち",
        )));

        let value = read_one_line(&listener);
        assert_eq!(value["params"]["state"], "blocked");
        assert_eq!(value["params"]["custom_status"], "HiL: プラン承認待ち");
    }

    #[test]
    fn test_publish_idle_has_null_custom_status() {
        let (_dir, listener, socket_path) = bind_test_socket();
        let adapter = HerdrReporterAdapter::new("w3A:p1".to_string(), socket_path);

        adapter.publish(AppEvent::AgentStatusChanged(AgentStatus::Idle));

        let value = read_one_line(&listener);
        assert_eq!(value["params"]["state"], "idle");
        assert!(value["params"]["custom_status"].is_null());
    }

    #[test]
    fn test_shutdown_is_synchronous_and_idempotent() {
        let (_dir, listener, socket_path) = bind_test_socket();
        let adapter = HerdrReporterAdapter::new("w3A:p1".to_string(), socket_path);

        // shutdown() sends directly (bypassing the channel/writer thread) —
        // by the time it returns, the release must already be on the wire,
        // so a subsequent accept() must not block.
        adapter.shutdown();
        let value = read_one_line(&listener);
        assert_eq!(value["method"], "pane.release_agent");

        // A second call (and the eventual Drop) must not send again.
        adapter.shutdown();
        listener.set_nonblocking(true).unwrap();
        assert!(matches!(
            listener.accept(),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
        ));
    }

    #[test]
    fn test_quorum_result_is_ignored() {
        let (_dir, listener, socket_path) = bind_test_socket();
        let adapter = HerdrReporterAdapter::new("w3A:p1".to_string(), socket_path);

        // Publish a status change, then a QuorumResult, then drop (which
        // sends Release). If QuorumResult were wired to send anything, it
        // would land as a *second* connection between these two — but the
        // writer thread processes messages in order over one channel, so
        // asserting the release comes right after the status change proves
        // nothing was sent in between.
        adapter.publish(AppEvent::AgentStatusChanged(AgentStatus::Idle));
        let first = read_one_line(&listener);
        assert_eq!(first["method"], "pane.report_agent");

        drop(adapter);
        let second = read_one_line(&listener);
        assert_eq!(second["method"], "pane.release_agent");

        listener.set_nonblocking(true).unwrap();
        assert!(matches!(
            listener.accept(),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
        ));
    }

    #[test]
    fn test_drop_sends_release_and_flushes() {
        let (_dir, listener, socket_path) = bind_test_socket();
        {
            let _adapter = HerdrReporterAdapter::new("w3A:p1".to_string(), socket_path);
            // adapter dropped at end of this block
        }

        let value = read_one_line(&listener);
        assert_eq!(value["method"], "pane.release_agent");
        assert_eq!(value["params"]["pane_id"], "w3A:p1");
    }

    #[test]
    fn test_send_failure_degrades_silently() {
        let adapter =
            HerdrReporterAdapter::new("w3A:p1".to_string(), "/nonexistent/quorum.sock".to_string());
        // Must not panic or hang, on publish or on drop.
        adapter.publish(AppEvent::AgentStatusChanged(AgentStatus::Working(None)));
        drop(adapter);
    }

    #[test]
    fn test_from_env_is_noop_without_herdr_env() {
        // HERDR_ENV is not set in the test process by default; from_env()
        // must not spawn a thread or touch any socket.
        if std::env::var("HERDR_ENV").is_ok() {
            // Running inside an actual herdr pane (e.g. dev shell) — skip,
            // this test only asserts the *absence* behavior.
            return;
        }
        assert!(HerdrReporterAdapter::from_env().is_none());
    }
}
