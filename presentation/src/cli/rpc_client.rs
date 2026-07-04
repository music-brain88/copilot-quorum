//! Built-in JSON-RPC client for the Remote Control API socket (`--listen`).
//!
//! `copilot-quorum rpc` (#302) — an installed binary needing neither a
//! repository checkout nor Python to drive a running TUI/headless instance.
//! Talks the exact same wire protocol as the reference client
//! `scripts/tui-rpc.py` (kept as a protocol example, not replaced): LSP-style
//! `Content-Length` framing + JSON-RPC 2.0, one request per connection.
//!
//! See `presentation/src/tui/remote.rs` for the server side.

use super::commands::RpcArgs;
use serde_json::Value;
use std::io;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// Send one JSON-RPC request over `socket` and return the full response
/// envelope (`{"jsonrpc", "id", "result"}` or `{"jsonrpc", "id", "error"}`).
async fn request(socket: &Path, method: &str, params: Value) -> io::Result<Value> {
    let mut stream = UnixStream::connect(socket).await.map_err(|e| {
        io::Error::new(
            e.kind(),
            format!(
                "could not connect to socket {} ({e}). Is a copilot-quorum instance \
                 running with `--listen {}`?",
                socket.display(),
                socket.display()
            ),
        )
    })?;

    let body = serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    }))
    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    stream
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await?;
    stream.write_all(&body).await?;

    let mut buf = Vec::new();
    let mut chunk = [0u8; 65536];
    loop {
        if let Some(header_end) = find_header_end(&buf) {
            let content_length = parse_content_length(&buf[..header_end])?;
            let total = header_end + 4 + content_length;
            while buf.len() < total {
                let n = stream.read(&mut chunk).await?;
                if n == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "socket closed before the response completed",
                    ));
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let body = &buf[header_end + 4..total];
            return serde_json::from_slice(body)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
        }
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "socket closed before any response header arrived",
            ));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Byte offset of the `\r\n\r\n` header/body separator, if present.
fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Parse the `Content-Length` header out of the (not yet UTF-8-validated)
/// header block.
fn parse_content_length(head: &[u8]) -> io::Result<usize> {
    let text = String::from_utf8_lossy(head);
    for line in text.lines() {
        if let Some(rest) = line
            .strip_prefix("Content-Length:")
            .or_else(|| line.strip_prefix("content-length:"))
        {
            return rest
                .trim()
                .parse::<usize>()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length"));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "missing Content-Length header",
    ))
}

/// Run `copilot-quorum rpc` end to end: parse `params`, send the request,
/// print the result (or error) to stdout/stderr. Returns the process exit
/// code (`0` on a JSON-RPC result, `1` on a JSON-RPC error).
pub async fn run_rpc(args: &RpcArgs) -> io::Result<i32> {
    let params: Value = match &args.params {
        Some(raw) => serde_json::from_str(raw).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid JSON params: {e}"),
            )
        })?,
        None => serde_json::json!({}),
    };

    let response = request(&args.socket, &args.method, params).await?;

    if let Some(result) = response.get("result") {
        println!("{}", serde_json::to_string_pretty(result).unwrap());
        Ok(0)
    } else {
        let error = response.get("error").cloned().unwrap_or(response);
        eprintln!("{}", serde_json::to_string_pretty(&error).unwrap());
        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::net::UnixListener;

    /// Minimal one-shot server mirroring `remote::handle_connection`'s wire
    /// format, so this test exercises the client against the real framing
    /// without pulling in the whole `TuiApp`.
    async fn serve_once(path: &Path, response_body: Value) {
        let listener = UnixListener::bind(path).unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let mut content_length = None;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let trimmed = line.trim();
            if trimmed.is_empty() {
                break;
            }
            if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
                content_length = rest.trim().parse::<usize>().ok();
            }
        }
        let mut body = vec![0u8; content_length.unwrap()];
        reader.read_exact(&mut body).await.unwrap();
        let _request: Value = serde_json::from_slice(&body).unwrap();

        let response_bytes = serde_json::to_vec(&response_body).unwrap();
        write_half
            .write_all(format!("Content-Length: {}\r\n\r\n", response_bytes.len()).as_bytes())
            .await
            .unwrap();
        write_half.write_all(&response_bytes).await.unwrap();
    }

    fn temp_socket_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "quorum-rpc-client-test-{name}-{}.sock",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn request_round_trips_a_result() {
        let path = temp_socket_path("result");
        let _ = std::fs::remove_file(&path);
        let server = tokio::spawn({
            let path = path.clone();
            async move {
                serve_once(
                    &path,
                    serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}}),
                )
                .await;
            }
        });

        // Give the listener a moment to bind before connecting.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let result = request(&path, "state.get", serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(result["result"]["ok"], true);

        server.await.unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn request_round_trips_an_error() {
        let path = temp_socket_path("error");
        let _ = std::fs::remove_file(&path);
        let server = tokio::spawn({
            let path = path.clone();
            async move {
                serve_once(
                    &path,
                    serde_json::json!({
                        "jsonrpc": "2.0", "id": 1,
                        "error": {"code": -32601, "message": "Method not found: bogus"}
                    }),
                )
                .await;
            }
        });

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let result = request(&path, "bogus", serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(result["error"]["code"], -32601);

        server.await.unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn connecting_to_a_missing_socket_gives_a_friendly_error() {
        let path = temp_socket_path("missing");
        let _ = std::fs::remove_file(&path);
        let err = request(&path, "state.get", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("--listen"));
    }
}
