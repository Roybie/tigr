//! `import 'Net'` — TCP / UDP / TLS networking (v0.15).
//!
//! A socket is a first-class `Value` (`Value::Socket`): `Arc`-backed
//! and `Send`, so it crosses an actor boundary. The idiom is one actor
//! per connection — `accept` a socket, then `spawn` a handler closure
//! that captures it.
//!
//! Reads come in two layers: the low-level `read(sock, n)` returns up
//! to `n` bytes (an empty `Bytes` means end-of-stream), and the
//! helpers `read_exact` / `read_line` / `read_until` / `read_all` build
//! framed reads on top of it — the socket carries an internal buffer
//! so a helper that over-reads keeps the surplus.
//!
//! Failures raise a catchable structured error `${kind, message}`;
//! `kind` is one of `timeout`, `closed`, `eof`, `refused`, `dns`,
//! `tls`, `addr_in_use`, `decode`, or `io`.

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::offload::{BlockingJob, OffloadErr, OffloadOk};
use crate::vm::socket::{self, NetError, SocketHandle};
use crate::vm::value::{Arity, Value};

use super::{native, native_blocking, object};

pub fn module() -> Value {
    object(&[
        // -- TCP --  (`listen` binds without waiting; `accept` and
        // `connect` wait, so they are offloaded inside a green thread.)
        ("listen",      native("listen",      Arity::Exact(2), n_listen)),
        ("accept",      native_blocking("accept",  Arity::Exact(1), n_accept)),
        ("connect",     native_blocking("connect", Arity::Exact(2), n_connect)),
        // -- TLS --
        ("connect_tls", native_blocking("connect_tls", Arity::Exact(2), n_connect_tls)),
        // -- UDP --
        ("bind",        native("bind",        Arity::Exact(2), n_bind)),
        ("send_to",     native_blocking("send_to",   Arity::Exact(4), n_send_to)),
        ("recv_from",   native_blocking("recv_from", Arity::Exact(2), n_recv_from)),
        // -- stream I/O --
        ("read",        native_blocking("read",       Arity::Exact(2), n_read)),
        ("write",       native_blocking("write",      Arity::Exact(2), n_write)),
        ("read_exact",  native_blocking("read_exact", Arity::Exact(2), n_read_exact)),
        ("read_line",   native_blocking("read_line",  Arity::Exact(1), n_read_line)),
        ("read_until",  native_blocking("read_until", Arity::Exact(2), n_read_until)),
        ("read_all",    native_blocking("read_all",   Arity::Exact(1), n_read_all)),
        // -- addressing & lifecycle (all non-waiting — stay inline) --
        ("local_addr",  native("local_addr",  Arity::Exact(1), n_local_addr)),
        ("peer_addr",   native("peer_addr",   Arity::Exact(1), n_peer_addr)),
        ("set_timeout", native("set_timeout", Arity::Exact(2), n_set_timeout)),
        ("close",       native("close",       Arity::Exact(1), n_close)),
    ])
}

// ---------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------

/// A catchable, string-valued error — used for argument-type misuse.
fn err(msg: String) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::Raised(Value::Str(msg.into())), 0)
}

/// A catchable structured error `${kind, message}`, so `catch` code
/// (and `Test.assert_raises(..., kind)`) can dispatch on `.kind`.
fn net_err(kind: &str, msg: String) -> RuntimeError {
    let obj = object(&[
        ("kind", Value::Str(kind.into())),
        ("message", Value::Str(msg.into())),
    ]);
    RuntimeError::new(RuntimeErrorKind::Raised(obj), 0)
}

/// Classify a [`NetError`] from the socket layer into a structured
/// error `kind` and a message. A `None` kind means a plain
/// string-valued error — a wrong-kind call (e.g. `read` on a listener)
/// is a program bug, not a runtime condition.
fn classify(label: &str, e: NetError) -> (Option<&'static str>, String) {
    match e {
        NetError::Closed => {
            (Some("closed"), format!("Net.{label}: socket is closed"))
        }
        NetError::WrongKind(msg) => (None, format!("Net.{label}: {msg}")),
        NetError::Dns(msg) => (Some("dns"), format!("Net.{label}: {msg}")),
        NetError::Tls(msg) => (Some("tls"), format!("Net.{label}: {msg}")),
        NetError::Io(io_err) => {
            let kind = match io_err.kind() {
                io::ErrorKind::ConnectionRefused => "refused",
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => "timeout",
                io::ErrorKind::AddrInUse => "addr_in_use",
                io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::BrokenPipe
                | io::ErrorKind::NotConnected => "closed",
                _ => "io",
            };
            (Some(kind), format!("Net.{label}: {io_err}"))
        }
    }
}

/// Map a [`NetError`] to a tigr `RuntimeError` — used by the inline
/// (non-offloaded) natives.
fn map_err(label: &str, e: NetError) -> RuntimeError {
    match classify(label, e) {
        (Some(kind), msg) => net_err(kind, msg),
        (None, msg) => err(msg),
    }
}

/// Map a [`NetError`] to an [`OffloadErr`] — the POD error a worker
/// thread posts back; [`crate::vm::offload::decode`] rebuilds the same
/// `${kind, message}` or string error the inline call would raise.
fn offload_err(label: &str, e: NetError) -> OffloadErr {
    let (kind, message) = classify(label, e);
    OffloadErr { kind: kind.map(|k| k.to_string()), message }
}

/// A worker-side structured error raised directly (not from a
/// `NetError`) — `Net.read_exact`'s `eof`, `Net.read_line`'s `decode`.
fn offload_net_err(kind: &str, message: String) -> OffloadErr {
    OffloadErr { kind: Some(kind.to_string()), message }
}

// ---------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------

/// Extract a socket handle, or raise a `type_mismatch`.
fn as_socket<'a>(
    v: &'a Value,
    label: &str,
) -> Result<&'a SocketHandle, RuntimeError> {
    match v {
        Value::Socket(s) => Ok(s),
        other => Err(RuntimeError::new(
            RuntimeErrorKind::TypeMismatch(format!(
                "Net.{label}: expected a socket, got {}",
                other.type_name()
            )),
            0,
        )),
    }
}

/// Extract an owned socket handle for a blocking native — the worker
/// closure outlives the borrowed `Value`. `SocketHandle` is an `Arc`,
/// so this is a cheap refcount bump.
fn take_socket(v: &Value, label: &str) -> Result<SocketHandle, RuntimeError> {
    as_socket(v, label).cloned()
}

fn expect_str<'a>(v: &'a Value, label: &str) -> Result<&'a str, RuntimeError> {
    match v {
        Value::Str(s) => Ok(s),
        other => Err(err(format!(
            "Net.{label}: expected String, got {}",
            other.type_name()
        ))),
    }
}

fn expect_int(v: &Value, label: &str) -> Result<i64, RuntimeError> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(err(format!(
            "Net.{label}: expected Int, got {}",
            other.type_name()
        ))),
    }
}

/// A `port` argument — an `Int` in `0..=65535`.
fn expect_port(v: &Value, label: &str) -> Result<u16, RuntimeError> {
    match expect_int(v, label)? {
        n if (0..=65535).contains(&n) => Ok(n as u16),
        n => Err(err(format!(
            "Net.{label}: port {n} out of range 0..=65535"
        ))),
    }
}

/// A non-negative byte-count argument.
fn expect_count(v: &Value, label: &str) -> Result<usize, RuntimeError> {
    match expect_int(v, label)? {
        n if n >= 0 => Ok(n as usize),
        n => Err(err(format!("Net.{label}: negative count {n}"))),
    }
}

/// A single byte (`Int` in `0..=255`).
fn expect_byte(v: &Value, label: &str) -> Result<u8, RuntimeError> {
    match expect_int(v, label)? {
        n if (0..=255).contains(&n) => Ok(n as u8),
        n => Err(err(format!(
            "Net.{label}: byte value {n} out of range 0..=255"
        ))),
    }
}

/// Snapshot a `Bytes` argument's contents.
fn expect_bytes(v: &Value, label: &str) -> Result<Vec<u8>, RuntimeError> {
    match v {
        Value::Bytes(b) => Ok(b.borrow().clone()),
        other => Err(err(format!(
            "Net.{label}: expected Bytes, got {}",
            other.type_name()
        ))),
    }
}

/// Build a `${host, port}` address object.
fn addr_object(addr: SocketAddr) -> Value {
    object(&[
        ("host", Value::Str(addr.ip().to_string().into())),
        ("port", Value::Int(addr.port() as i64)),
    ])
}

// ---------------------------------------------------------------------
// TCP
// ---------------------------------------------------------------------

/// `listen(host, port)` — a TCP listener bound to `host:port`. Pass
/// port `0` to let the OS choose; read it back with `local_addr`.
fn n_listen(args: &[Value]) -> Result<Value, RuntimeError> {
    let host = expect_str(&args[0], "listen")?;
    let port = expect_port(&args[1], "listen")?;
    let sock = socket::listen(host, port).map_err(|e| map_err("listen", e))?;
    Ok(Value::Socket(sock))
}

/// `accept(listener)` — block for the next inbound connection.
/// Raises `closed` if the listener is closed, including by another
/// actor while this `accept` is waiting.
fn n_accept(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let sock = take_socket(&args[0], "accept")?;
    Ok(Box::new(move || match sock.accept() {
        Ok(conn) => Ok(OffloadOk::Socket(conn)),
        Err(e) => Err(offload_err("accept", e)),
    }))
}

/// `connect(host, port)` — open a TCP stream to `host:port`.
fn n_connect(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let host = expect_str(&args[0], "connect")?.to_string();
    let port = expect_port(&args[1], "connect")?;
    Ok(Box::new(move || match socket::connect(&host, port) {
        Ok(sock) => Ok(OffloadOk::Socket(sock)),
        Err(e) => Err(offload_err("connect", e)),
    }))
}

// ---------------------------------------------------------------------
// TLS
// ---------------------------------------------------------------------

/// `connect_tls(host, port)` — open a TLS-encrypted stream. `host` is
/// also the name verified against the server certificate.
fn n_connect_tls(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let host = expect_str(&args[0], "connect_tls")?.to_string();
    let port = expect_port(&args[1], "connect_tls")?;
    Ok(Box::new(move || match socket::connect_tls(&host, port) {
        Ok(sock) => Ok(OffloadOk::Socket(sock)),
        Err(e) => Err(offload_err("connect_tls", e)),
    }))
}

// ---------------------------------------------------------------------
// UDP
// ---------------------------------------------------------------------

/// `bind(host, port)` — a UDP datagram socket bound to `host:port`.
fn n_bind(args: &[Value]) -> Result<Value, RuntimeError> {
    let host = expect_str(&args[0], "bind")?;
    let port = expect_port(&args[1], "bind")?;
    let sock = socket::udp_bind(host, port).map_err(|e| map_err("bind", e))?;
    Ok(Value::Socket(sock))
}

/// `send_to(sock, bytes, host, port)` — send one UDP datagram; returns
/// the number of bytes sent.
fn n_send_to(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let sock = take_socket(&args[0], "send_to")?;
    let data = expect_bytes(&args[1], "send_to")?;
    let host = expect_str(&args[2], "send_to")?.to_string();
    let port = expect_port(&args[3], "send_to")?;
    Ok(Box::new(move || {
        // DNS resolution waits too — keep it on the worker thread.
        let addr = match socket::resolve(&host, port) {
            Ok(a) => a,
            Err(e) => return Err(offload_err("send_to", e)),
        };
        match sock.send_to(&data, addr) {
            Ok(sent) => Ok(OffloadOk::Int(sent as i64)),
            Err(e) => Err(offload_err("send_to", e)),
        }
    }))
}

/// `recv_from(sock, n)` — receive one UDP datagram (up to `n` bytes).
/// Returns `${data: Bytes, host: String, port: Int}`.
fn n_recv_from(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let sock = take_socket(&args[0], "recv_from")?;
    let n = expect_count(&args[1], "recv_from")?;
    Ok(Box::new(move || match sock.recv_from(n) {
        Ok((data, addr)) => Ok(OffloadOk::RecvFrom {
            data,
            host: addr.ip().to_string(),
            port: addr.port(),
        }),
        Err(e) => Err(offload_err("recv_from", e)),
    }))
}

// ---------------------------------------------------------------------
// Stream I/O
// ---------------------------------------------------------------------

/// `read(sock, n)` — read up to `n` bytes. An empty `Bytes` means the
/// stream has ended.
fn n_read(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let sock = take_socket(&args[0], "read")?;
    let n = expect_count(&args[1], "read")?;
    Ok(Box::new(move || match sock.read_chunk(n) {
        Ok(data) => Ok(OffloadOk::Bytes(data)),
        Err(e) => Err(offload_err("read", e)),
    }))
}

/// `write(sock, bytes)` — write every byte; returns the count written.
fn n_write(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let sock = take_socket(&args[0], "write")?;
    let data = expect_bytes(&args[1], "write")?;
    Ok(Box::new(move || match sock.write_all(&data) {
        Ok(()) => Ok(OffloadOk::Int(data.len() as i64)),
        Err(e) => Err(offload_err("write", e)),
    }))
}

/// `read_exact(sock, n)` — read exactly `n` bytes, blocking until they
/// arrive. Raises `eof` if the stream ends first.
fn n_read_exact(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let sock = take_socket(&args[0], "read_exact")?;
    let n = expect_count(&args[1], "read_exact")?;
    Ok(Box::new(move || {
        let mut out: Vec<u8> = Vec::new();
        while out.len() < n {
            let chunk = match sock.read_chunk(n - out.len()) {
                Ok(c) => c,
                Err(e) => return Err(offload_err("read_exact", e)),
            };
            if chunk.is_empty() {
                return Err(offload_net_err(
                    "eof",
                    format!(
                        "Net.read_exact: stream ended after {} of {n} bytes",
                        out.len()
                    ),
                ));
            }
            out.extend(chunk);
        }
        Ok(OffloadOk::Bytes(out))
    }))
}

/// `read_line(sock)` — read one `\n`-terminated line as a String, with
/// the trailing `\r\n` / `\n` stripped. Returns `null` at end-of-
/// stream. Raises `decode` on invalid UTF-8.
fn n_read_line(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let sock = take_socket(&args[0], "read_line")?;
    Ok(Box::new(move || {
        let line = match sock.read_until(b'\n') {
            Ok(l) => l,
            Err(e) => return Err(offload_err("read_line", e)),
        };
        match line {
            None => Ok(OffloadOk::StrOrNull(None)),
            Some(mut line) => {
                if line.last() == Some(&b'\n') {
                    line.pop();
                }
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                match String::from_utf8(line) {
                    Ok(s) => Ok(OffloadOk::StrOrNull(Some(s))),
                    Err(e) => Err(offload_net_err(
                        "decode",
                        format!(
                            "Net.read_line: invalid UTF-8 at byte {}",
                            e.utf8_error().valid_up_to()
                        ),
                    )),
                }
            }
        }
    }))
}

/// `read_until(sock, byte)` — read up to and including the next `byte`.
/// Returns a `Bytes` (the delimiter included), or `null` at end-of-
/// stream. Trailing data with no delimiter is returned as a final
/// chunk.
fn n_read_until(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let sock = take_socket(&args[0], "read_until")?;
    let delim = expect_byte(&args[1], "read_until")?;
    Ok(Box::new(move || match sock.read_until(delim) {
        Ok(opt) => Ok(OffloadOk::BytesOrNull(opt)),
        Err(e) => Err(offload_err("read_until", e)),
    }))
}

/// `read_all(sock)` — read every remaining byte until end-of-stream.
fn n_read_all(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let sock = take_socket(&args[0], "read_all")?;
    Ok(Box::new(move || {
        let mut out: Vec<u8> = Vec::new();
        loop {
            let chunk = match sock.read_chunk(65536) {
                Ok(c) => c,
                Err(e) => return Err(offload_err("read_all", e)),
            };
            if chunk.is_empty() {
                break;
            }
            out.extend(chunk);
        }
        Ok(OffloadOk::Bytes(out))
    }))
}

// ---------------------------------------------------------------------
// Addressing & lifecycle
// ---------------------------------------------------------------------

/// `local_addr(sock)` — the socket's own bound address.
fn n_local_addr(args: &[Value]) -> Result<Value, RuntimeError> {
    let sock = as_socket(&args[0], "local_addr")?;
    let addr = sock.local_addr().map_err(|e| map_err("local_addr", e))?;
    Ok(addr_object(addr))
}

/// `peer_addr(sock)` — the connected peer's address.
fn n_peer_addr(args: &[Value]) -> Result<Value, RuntimeError> {
    let sock = as_socket(&args[0], "peer_addr")?;
    let addr = sock.peer_addr().map_err(|e| map_err("peer_addr", e))?;
    Ok(addr_object(addr))
}

/// `set_timeout(sock, ms)` — bound subsequent reads and writes to `ms`
/// milliseconds; a timed-out operation raises `timeout`. `ms <= 0`
/// clears the timeout (block indefinitely).
fn n_set_timeout(args: &[Value]) -> Result<Value, RuntimeError> {
    let sock = as_socket(&args[0], "set_timeout")?;
    let ms = expect_int(&args[1], "set_timeout")?;
    let dur = if ms > 0 {
        Some(Duration::from_millis(ms as u64))
    } else {
        None
    };
    sock.set_timeout(dur).map_err(|e| map_err("set_timeout", e))?;
    Ok(Value::Null)
}

/// `close(sock)` — close the socket. Idempotent; unblocks a reader
/// stuck mid-`read`, and an actor stuck in `accept` on a listener
/// (which then raises `closed`).
fn n_close(args: &[Value]) -> Result<Value, RuntimeError> {
    let sock = as_socket(&args[0], "close")?;
    sock.close();
    Ok(Value::Null)
}
