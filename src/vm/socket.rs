//! Network socket — the handle behind the v0.15 `Net` module.
//!
//! A `Socket` is `Arc`-shared and `Send + Sync`, like a `Channel` or a
//! `Task`: it lives outside any heap (a GC leaf) and crosses actor
//! threads by handle-clone via [`crate::vm::transfer::Transfer::Socket`].
//! That is what lets the per-connection-actor idiom work — `accept` a
//! connection, then `spawn` a handler closure that captures the socket.
//!
//! ## Locking
//!
//! [`SocketKind`] is fixed at construction and never mutated, so no
//! lock guards the *kind* — a blocking read therefore never holds a
//! lock that `close` needs. Each connected stream is split into
//! independent read / write halves (via `TcpStream::try_clone`) behind
//! their own mutexes, so a reader actor and a writer actor never
//! contend. `close` records a flag and fires `shutdown` on a third,
//! never-locked clone, which unblocks a reader stuck mid-`read`.
//!
//! ## Read buffer
//!
//! [`SocketInner::read_until`] over-reads past its delimiter; the
//! surplus is kept in `read_buf` so the next read sees it first. The
//! buffer lock is never held across a blocking syscall.

use std::io::{self, Read, Write};
use std::net::{
    Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket,
};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

/// A shared, `Send` socket handle. Cloning bumps the `Arc` refcount.
pub type SocketHandle = Arc<SocketInner>;

/// A blocking TLS client stream over a plain TCP connection.
type TlsStream = StreamOwned<ClientConnection, TcpStream>;

/// How long `connect` / `connect_tls` wait for the TCP handshake before
/// giving up — a bounded default, since neither takes a timeout arg.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Per-syscall read size for the delimiter-scanning helper.
const CHUNK: usize = 8192;
/// Cap on a single `read_chunk` syscall — a huge `n` must not allocate
/// gigabytes up front. A short read is valid; the caller loops.
const MAX_DIRECT_READ: usize = 65536;
/// Cap on a single UDP datagram receive buffer.
const MAX_DATAGRAM: usize = 65536;
/// Poll cadence for an interruptible `accept` — the wait between
/// retries while no connection is pending. A listener has no
/// `shutdown`, so `close` cannot wake a blocked `accept` directly;
/// instead `accept` polls a non-blocking listener and observes the
/// `closed` flag within this bound. It only sleeps while *idle* —
/// queued connections are returned at once.
const ACCEPT_POLL: Duration = Duration::from_millis(50);

/// An operation failure, mapped to a structured tigr error by `net.rs`.
pub enum NetError {
    /// The socket was `close`d.
    Closed,
    /// The operation does not apply to this kind of socket (e.g.
    /// `read` on a listener) — a programming error.
    WrongKind(String),
    /// Host / port could not be resolved to an address.
    Dns(String),
    /// A TLS handshake or certificate failure.
    Tls(String),
    /// Any other I/O error; `net.rs` refines it by `io::ErrorKind`.
    Io(io::Error),
}

impl From<io::Error> for NetError {
    fn from(e: io::Error) -> Self {
        NetError::Io(e)
    }
}

/// The transport behind a socket. Set once at construction.
enum SocketKind {
    /// A listening TCP server socket. `accept` takes `&self`.
    TcpListener(TcpListener),
    /// A connected TCP stream, split into independently-locked halves.
    TcpStream { read: Mutex<TcpStream>, write: Mutex<TcpStream> },
    /// A UDP datagram socket. Its methods take `&self`.
    Udp(UdpSocket),
    /// A connected TLS client stream — not split (rustls keeps a single
    /// stateful connection), so one lock covers both directions.
    Tls(Mutex<Box<TlsStream>>),
}

pub struct SocketInner {
    kind: SocketKind,
    /// Set by `close`; every operation checks it and raises `Closed`.
    closed: AtomicBool,
    /// A spare `TcpStream` clone held outside every I/O lock, used only
    /// by `close` to fire `shutdown(Both)` — that unblocks a reader
    /// stuck on a different clone. `None` for listeners / UDP.
    shutdown: Mutex<Option<TcpStream>>,
    /// Surplus bytes read past what `read_until` needed.
    read_buf: Mutex<Vec<u8>>,
    /// Stable id for legible display; identity is `Arc::ptr_eq`.
    id: u64,
}

fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

// ---------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------

/// Resolve `host:port` to a single socket address.
pub fn resolve(host: &str, port: u16) -> Result<SocketAddr, NetError> {
    (host, port)
        .to_socket_addrs()
        .map_err(|e| NetError::Dns(format!("{host}:{port}: {e}")))?
        .next()
        .ok_or_else(|| NetError::Dns(format!("{host}:{port}: no address resolved")))
}

/// Wrap a connected `TcpStream` in a socket, splitting read / write /
/// shutdown handles.
fn tcp_stream_socket(stream: TcpStream) -> Result<SocketHandle, NetError> {
    let read = stream.try_clone()?;
    let write = stream.try_clone()?;
    Ok(Arc::new(SocketInner {
        kind: SocketKind::TcpStream {
            read: Mutex::new(read),
            write: Mutex::new(write),
        },
        closed: AtomicBool::new(false),
        // `stream` itself becomes the close-only shutdown handle.
        shutdown: Mutex::new(Some(stream)),
        read_buf: Mutex::new(Vec::new()),
        id: next_id(),
    }))
}

/// Bind a listening TCP socket.
pub fn listen(host: &str, port: u16) -> Result<SocketHandle, NetError> {
    let listener = TcpListener::bind((host, port))?;
    // Non-blocking so `accept` can poll the `closed` flag and stay
    // interruptible — see `SocketInner::accept`.
    listener.set_nonblocking(true)?;
    Ok(Arc::new(SocketInner {
        kind: SocketKind::TcpListener(listener),
        closed: AtomicBool::new(false),
        shutdown: Mutex::new(None),
        read_buf: Mutex::new(Vec::new()),
        id: next_id(),
    }))
}

/// Open a TCP connection to `host:port`.
pub fn connect(host: &str, port: u16) -> Result<SocketHandle, NetError> {
    let addr = resolve(host, port)?;
    let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
    tcp_stream_socket(stream)
}

/// Bind a UDP datagram socket.
pub fn udp_bind(host: &str, port: u16) -> Result<SocketHandle, NetError> {
    let socket = UdpSocket::bind((host, port))?;
    Ok(Arc::new(SocketInner {
        kind: SocketKind::Udp(socket),
        closed: AtomicBool::new(false),
        shutdown: Mutex::new(None),
        read_buf: Mutex::new(Vec::new()),
        id: next_id(),
    }))
}

/// The shared TLS client config — native trust roots, built once.
fn tls_config() -> Arc<ClientConfig> {
    static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = RootCertStore::empty();
            let loaded = rustls_native_certs::load_native_certs();
            roots.add_parsable_certificates(loaded.certs);
            let provider = rustls::crypto::ring::default_provider();
            let config = ClientConfig::builder_with_provider(provider.into())
                .with_safe_default_protocol_versions()
                .expect("ring provider supports the default TLS versions")
                .with_root_certificates(roots)
                .with_no_client_auth();
            Arc::new(config)
        })
        .clone()
}

/// Open a TLS-encrypted TCP connection to `host:port`. `host` doubles
/// as the certificate-verification server name.
pub fn connect_tls(host: &str, port: u16) -> Result<SocketHandle, NetError> {
    let addr = resolve(host, port)?;
    let tcp = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
    let shutdown = tcp.try_clone()?;
    let server_name = ServerName::try_from(host.to_string())
        .map_err(|e| NetError::Tls(format!("invalid server name '{host}': {e}")))?;
    let conn = ClientConnection::new(tls_config(), server_name)
        .map_err(|e| NetError::Tls(e.to_string()))?;
    let tls = StreamOwned::new(conn, tcp);
    Ok(Arc::new(SocketInner {
        kind: SocketKind::Tls(Mutex::new(Box::new(tls))),
        closed: AtomicBool::new(false),
        shutdown: Mutex::new(Some(shutdown)),
        read_buf: Mutex::new(Vec::new()),
        id: next_id(),
    }))
}

// ---------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------

impl SocketInner {
    /// Stable identity number, for display.
    pub fn id(&self) -> u64 {
        self.id
    }

    fn ensure_open(&self) -> Result<(), NetError> {
        if self.closed.load(Ordering::Acquire) {
            Err(NetError::Closed)
        } else {
            Ok(())
        }
    }

    /// One blocking `read` syscall on the connected-stream halves.
    fn io_read_into(&self, buf: &mut [u8]) -> Result<usize, NetError> {
        match &self.kind {
            SocketKind::TcpStream { read, .. } => {
                let guard = read.lock().unwrap();
                let mut stream: &TcpStream = &guard;
                Ok(stream.read(buf)?)
            }
            SocketKind::Tls(m) => {
                let mut tls = m.lock().unwrap();
                Ok(tls.read(buf)?)
            }
            SocketKind::TcpListener(_) | SocketKind::Udp(_) => Err(
                NetError::WrongKind("read expects a connected stream".into()),
            ),
        }
    }

    /// Read up to `n` bytes. An empty result means end-of-stream. The
    /// internal buffer is drained first; otherwise a single syscall
    /// runs (capped — a short read is valid, the caller loops).
    pub fn read_chunk(&self, n: usize) -> Result<Vec<u8>, NetError> {
        self.ensure_open()?;
        if n == 0 {
            return Ok(Vec::new());
        }
        {
            let mut buf = self.read_buf.lock().unwrap();
            if !buf.is_empty() {
                let k = n.min(buf.len());
                return Ok(buf.drain(..k).collect());
            }
        }
        let mut tmp = vec![0u8; n.min(MAX_DIRECT_READ)];
        let got = self.io_read_into(&mut tmp)?;
        tmp.truncate(got);
        Ok(tmp)
    }

    /// Append one syscall's worth of bytes to `read_buf`; returns the
    /// count added (0 = end-of-stream). The buffer lock is not held
    /// across the syscall.
    fn recv_more(&self) -> Result<usize, NetError> {
        let mut tmp = vec![0u8; CHUNK];
        let got = self.io_read_into(&mut tmp)?;
        if got > 0 {
            self.read_buf.lock().unwrap().extend_from_slice(&tmp[..got]);
        }
        Ok(got)
    }

    /// Read up to and including the next `delim` byte. `None` means a
    /// clean end-of-stream with nothing buffered; trailing bytes with
    /// no delimiter at EOF are returned as a final unterminated chunk.
    pub fn read_until(&self, delim: u8) -> Result<Option<Vec<u8>>, NetError> {
        self.ensure_open()?;
        loop {
            {
                let mut buf = self.read_buf.lock().unwrap();
                if let Some(pos) = buf.iter().position(|&b| b == delim) {
                    return Ok(Some(buf.drain(..=pos).collect()));
                }
            }
            if self.recv_more()? == 0 {
                let mut buf = self.read_buf.lock().unwrap();
                return Ok(if buf.is_empty() {
                    None
                } else {
                    Some(std::mem::take(&mut *buf))
                });
            }
        }
    }

    /// Write every byte, then flush.
    pub fn write_all(&self, data: &[u8]) -> Result<(), NetError> {
        self.ensure_open()?;
        match &self.kind {
            SocketKind::TcpStream { write, .. } => {
                let guard = write.lock().unwrap();
                let mut stream: &TcpStream = &guard;
                stream.write_all(data)?;
                stream.flush()?;
                Ok(())
            }
            SocketKind::Tls(m) => {
                let mut tls = m.lock().unwrap();
                tls.write_all(data)?;
                tls.flush()?;
                Ok(())
            }
            SocketKind::TcpListener(_) | SocketKind::Udp(_) => Err(
                NetError::WrongKind("write expects a connected stream".into()),
            ),
        }
    }

    /// Accept the next inbound connection on a listener socket.
    /// Block for the next inbound connection. The listener is
    /// non-blocking, so this polls — observing a concurrent `close`
    /// within `ACCEPT_POLL` and raising `Closed` — rather than parking
    /// in a syscall that `close` could not interrupt.
    pub fn accept(&self) -> Result<SocketHandle, NetError> {
        match &self.kind {
            SocketKind::TcpListener(l) => loop {
                if self.closed.load(Ordering::Acquire) {
                    return Err(NetError::Closed);
                }
                match l.accept() {
                    Ok((stream, _addr)) => {
                        // The listener is non-blocking; hand back a
                        // blocking stream (its clones inherit the mode).
                        stream.set_nonblocking(false)?;
                        return tcp_stream_socket(stream);
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(ACCEPT_POLL);
                    }
                    Err(e) => return Err(e.into()),
                }
            },
            _ => Err(NetError::WrongKind("accept expects a listener".into())),
        }
    }

    /// Send a UDP datagram to `addr`; returns the byte count sent.
    pub fn send_to(&self, data: &[u8], addr: SocketAddr) -> Result<usize, NetError> {
        self.ensure_open()?;
        match &self.kind {
            SocketKind::Udp(u) => Ok(u.send_to(data, addr)?),
            _ => Err(NetError::WrongKind("send_to expects a UDP socket".into())),
        }
    }

    /// Receive a UDP datagram (up to `n` bytes) and its sender address.
    pub fn recv_from(&self, n: usize) -> Result<(Vec<u8>, SocketAddr), NetError> {
        self.ensure_open()?;
        match &self.kind {
            SocketKind::Udp(u) => {
                let mut tmp = vec![0u8; n.min(MAX_DATAGRAM)];
                let (got, addr) = u.recv_from(&mut tmp)?;
                tmp.truncate(got);
                Ok((tmp, addr))
            }
            _ => Err(NetError::WrongKind("recv_from expects a UDP socket".into())),
        }
    }

    /// The socket's own bound address.
    pub fn local_addr(&self) -> Result<SocketAddr, NetError> {
        Ok(match &self.kind {
            SocketKind::TcpListener(l) => l.local_addr()?,
            SocketKind::TcpStream { read, .. } => {
                read.lock().unwrap().local_addr()?
            }
            SocketKind::Udp(u) => u.local_addr()?,
            SocketKind::Tls(m) => m.lock().unwrap().sock.local_addr()?,
        })
    }

    /// The address of the connected peer.
    pub fn peer_addr(&self) -> Result<SocketAddr, NetError> {
        match &self.kind {
            SocketKind::TcpStream { read, .. } => {
                Ok(read.lock().unwrap().peer_addr()?)
            }
            SocketKind::Tls(m) => Ok(m.lock().unwrap().sock.peer_addr()?),
            SocketKind::TcpListener(_) | SocketKind::Udp(_) => Err(
                NetError::WrongKind("peer_addr expects a connected stream".into()),
            ),
        }
    }

    /// Set (or with `None`, clear) the read and write timeouts.
    pub fn set_timeout(&self, dur: Option<Duration>) -> Result<(), NetError> {
        match &self.kind {
            SocketKind::TcpStream { read, write } => {
                read.lock().unwrap().set_read_timeout(dur)?;
                write.lock().unwrap().set_write_timeout(dur)?;
                Ok(())
            }
            SocketKind::Udp(u) => {
                u.set_read_timeout(dur)?;
                u.set_write_timeout(dur)?;
                Ok(())
            }
            SocketKind::Tls(m) => {
                let tls = m.lock().unwrap();
                tls.sock.set_read_timeout(dur)?;
                tls.sock.set_write_timeout(dur)?;
                Ok(())
            }
            SocketKind::TcpListener(_) => Err(NetError::WrongKind(
                "set_timeout is not supported on a listener socket".into(),
            )),
        }
    }

    /// Close the socket. Idempotent. Fires `shutdown` on the spare
    /// handle so a reader blocked mid-`read` wakes and observes EOF.
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        if let Some(stream) = self.shutdown.lock().unwrap().take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Socket` crosses actor threads, so its handle must be `Send`
    /// (and `Sync`, since several actors may hold clones). Compile-time.
    #[test]
    fn socket_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SocketInner>();
        assert_send_sync::<SocketHandle>();
        assert_send_sync::<NetError>();
    }
}
