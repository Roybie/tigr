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
use crate::vm::offload::{BlockingJob, OffloadErr, OffloadOk, OffloadResult};
use crate::vm::socket::{self, NetError, ReactorOp, SocketHandle, SocketOp};
use crate::vm::value::{Arity, Value};

use super::{native, native_blocking, native_socket, object};

pub fn module() -> Value {
    object(&[
        // -- TCP --  (`listen` binds without waiting. `accept` is
        // driven on the async-IO reactor inside a green thread;
        // `connect` waits on the worker pool — DNS has no async form.)
        ("listen",      native("listen",      Arity::Exact(2), n_listen)),
        ("accept",      native_socket("accept",    Arity::Exact(1), n_accept)),
        ("connect",     native_blocking("connect", Arity::Exact(2), n_connect)),
        // -- TLS --  (`listen_tls` binds without waiting — the PEM
        // parse is CPU-only; `connect_tls` waits on the worker pool.)
        ("listen_tls",  native("listen_tls",  Arity::Exact(4), n_listen_tls)),
        ("connect_tls", native_blocking("connect_tls", Arity::Range(2, 3), n_connect_tls)),
        // -- UDP --  (`recv_from` waits for a datagram — reactor-driven;
        // `send_to` resolves its target with DNS, so it stays pooled.)
        ("bind",        native("bind",        Arity::Exact(2), n_bind)),
        ("send_to",     native_blocking("send_to", Arity::Exact(4), n_send_to)),
        ("recv_from",   native_socket("recv_from", Arity::Exact(2), n_recv_from)),
        // -- stream I/O --  (steady-state socket reads / writes — driven
        // on the async-IO reactor inside a green thread; see
        // `crate::vm::reactor`.)
        ("read",        native_socket("read",       Arity::Exact(2), n_read)),
        ("write",       native_socket("write",      Arity::Exact(2), n_write)),
        ("read_exact",  native_socket("read_exact", Arity::Exact(2), n_read_exact)),
        ("read_line",   native_socket("read_line",  Arity::Exact(1), n_read_line)),
        ("read_until",  native_socket("read_until", Arity::Exact(2), n_read_until)),
        ("read_all",    native_socket("read_all",   Arity::Exact(1), n_read_all)),
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
/// thread (or the reactor thread) posts back;
/// [`crate::vm::offload::decode`] rebuilds the same `${kind, message}`
/// or string error the inline call would raise.
pub(crate) fn offload_err(label: &str, e: NetError) -> OffloadErr {
    let (kind, message) = classify(label, e);
    OffloadErr { kind: kind.map(|k| k.to_string()), message }
}

/// An off-thread structured error raised directly (not from a
/// `NetError`) — `Net.read_exact`'s `eof`, `Net.read_line`'s `decode`.
pub(crate) fn offload_net_err(kind: &str, message: String) -> OffloadErr {
    OffloadErr { kind: Some(kind.to_string()), message }
}

/// Turn the raw bytes of one `read_until(b'\n')` into a `read_line`
/// result: strip the trailing `\r\n` / `\n`, decode UTF-8. `None`
/// (end-of-stream) decodes to `null`; invalid UTF-8 raises `decode`.
/// Shared by the inline executor and the reactor's `ReadLine` op.
pub(crate) fn finish_line(line: Option<Vec<u8>>) -> OffloadResult {
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

/// `accept(listener)` — wait for the next inbound connection. Raises
/// `closed` if the listener is closed, including by another actor
/// while this `accept` is waiting.
fn n_accept(args: &[Value]) -> Result<ReactorOp, RuntimeError> {
    let socket = take_socket(&args[0], "accept")?;
    Ok(ReactorOp { socket, op: SocketOp::Accept, label: "accept" })
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

/// `listen_tls(host, port, cert_pem, key_pem)` — a TLS server listener
/// bound to `host:port`. `cert_pem` / `key_pem` are PEM *content*
/// (certificate chain and private key), not file paths. `accept` on the
/// returned listener yields server-side TLS sockets, so
/// `Http.serve(Net.listen_tls(...), handler)` is an HTTPS server.
fn n_listen_tls(args: &[Value]) -> Result<Value, RuntimeError> {
    let host = expect_str(&args[0], "listen_tls")?;
    let port = expect_port(&args[1], "listen_tls")?;
    let cert = expect_str(&args[2], "listen_tls")?;
    let key = expect_str(&args[3], "listen_tls")?;
    let sock = socket::listen_tls(host, port, cert.as_bytes(), key.as_bytes())
        .map_err(|e| map_err("listen_tls", e))?;
    Ok(Value::Socket(sock))
}

/// `connect_tls(host, port, [ca_pem])` — open a TLS-encrypted stream.
/// `host` is also the name verified against the server certificate. The
/// optional `ca_pem` adds trusted root certificates beyond the OS trust
/// store — needed to connect to a private-CA or self-signed service.
fn n_connect_tls(args: &[Value]) -> Result<BlockingJob, RuntimeError> {
    let host = expect_str(&args[0], "connect_tls")?.to_string();
    let port = expect_port(&args[1], "connect_tls")?;
    let ca: Option<String> = match args.get(2) {
        None | Some(Value::Null) => None,
        Some(v) => Some(expect_str(v, "connect_tls")?.to_string()),
    };
    Ok(Box::new(move || {
        let extra = ca.as_deref().map(str::as_bytes);
        match socket::connect_tls(&host, port, extra) {
            Ok(sock) => Ok(OffloadOk::Socket(sock)),
            Err(e) => Err(offload_err("connect_tls", e)),
        }
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
fn n_recv_from(args: &[Value]) -> Result<ReactorOp, RuntimeError> {
    let socket = take_socket(&args[0], "recv_from")?;
    let n = expect_count(&args[1], "recv_from")?;
    Ok(ReactorOp {
        socket,
        op: SocketOp::RecvFrom(n),
        label: "recv_from",
    })
}

// ---------------------------------------------------------------------
// Stream I/O
// ---------------------------------------------------------------------

/// `read(sock, n)` — read up to `n` bytes. An empty `Bytes` means the
/// stream has ended.
fn n_read(args: &[Value]) -> Result<ReactorOp, RuntimeError> {
    let socket = take_socket(&args[0], "read")?;
    let n = expect_count(&args[1], "read")?;
    Ok(ReactorOp { socket, op: SocketOp::ReadChunk(n), label: "read" })
}

/// `write(sock, bytes)` — write every byte; returns the count written.
fn n_write(args: &[Value]) -> Result<ReactorOp, RuntimeError> {
    let socket = take_socket(&args[0], "write")?;
    let data = expect_bytes(&args[1], "write")?;
    Ok(ReactorOp {
        socket,
        op: SocketOp::WriteAll { data, sent: 0 },
        label: "write",
    })
}

/// `read_exact(sock, n)` — read exactly `n` bytes, waiting until they
/// arrive. Raises `eof` if the stream ends first.
fn n_read_exact(args: &[Value]) -> Result<ReactorOp, RuntimeError> {
    let socket = take_socket(&args[0], "read_exact")?;
    let n = expect_count(&args[1], "read_exact")?;
    Ok(ReactorOp {
        socket,
        op: SocketOp::ReadExact { need: n, got: Vec::new() },
        label: "read_exact",
    })
}

/// `read_line(sock)` — read one `\n`-terminated line as a String, with
/// the trailing `\r\n` / `\n` stripped. Returns `null` at end-of-
/// stream. Raises `decode` on invalid UTF-8.
fn n_read_line(args: &[Value]) -> Result<ReactorOp, RuntimeError> {
    let socket = take_socket(&args[0], "read_line")?;
    Ok(ReactorOp { socket, op: SocketOp::ReadLine, label: "read_line" })
}

/// `read_until(sock, byte)` — read up to and including the next `byte`.
/// Returns a `Bytes` (the delimiter included), or `null` at end-of-
/// stream. Trailing data with no delimiter is returned as a final
/// chunk.
fn n_read_until(args: &[Value]) -> Result<ReactorOp, RuntimeError> {
    let socket = take_socket(&args[0], "read_until")?;
    let delim = expect_byte(&args[1], "read_until")?;
    Ok(ReactorOp {
        socket,
        op: SocketOp::ReadUntil(delim),
        label: "read_until",
    })
}

/// `read_all(sock)` — read every remaining byte until end-of-stream.
fn n_read_all(args: &[Value]) -> Result<ReactorOp, RuntimeError> {
    let socket = take_socket(&args[0], "read_all")?;
    Ok(ReactorOp {
        socket,
        op: SocketOp::ReadAll(Vec::new()),
        label: "read_all",
    })
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
/// (which then raises `closed`). A coroutine parked on a reactor op
/// for this socket is woken with a catchable `closed` error.
fn n_close(args: &[Value]) -> Result<Value, RuntimeError> {
    let sock = as_socket(&args[0], "close")?;
    sock.close();
    crate::vm::reactor::cancel(sock.id());
    Ok(Value::Null)
}
