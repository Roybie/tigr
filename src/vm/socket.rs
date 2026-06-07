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
#[cfg(unix)]
use std::os::unix::io::{AsRawFd, RawFd};
#[cfg(windows)]
use std::os::windows::io::{AsRawSocket, RawSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{
    ClientConfig, ClientConnection, Connection, RootCertStore, ServerConfig,
    ServerConnection,
};

/// A shared, `Send` socket handle. Cloning bumps the `Arc` refcount.
pub type SocketHandle = Arc<SocketInner>;

/// The OS-level handle the reactor registers with `polling`: a file
/// descriptor on unix, a socket handle on Windows. `read_raw_handle` /
/// `write_raw_handle` hand these to [`crate::vm::reactor`].
#[cfg(unix)]
pub type RawHandle = RawFd;
#[cfg(windows)]
pub type RawHandle = RawSocket;

/// The raw OS handle of a `std::net` socket, spelled per platform.
#[cfg(unix)]
fn raw_handle_of<T: AsRawFd>(sock: &T) -> RawHandle {
    sock.as_raw_fd()
}
#[cfg(windows)]
fn raw_handle_of<T: AsRawSocket>(sock: &T) -> RawHandle {
    sock.as_raw_socket()
}

/// A TLS connection in unbundled form: the sans-IO `rustls` state
/// machine plus the raw `TcpStream` it drives. The reactor sets `sock`
/// non-blocking and hand-drives `rustls` ([`tls_read`] / [`tls_write`]);
/// the inline executor drives the same loop against a blocking `sock`.
/// One `Mutex` covers the pair on [`SocketKind::Tls`] — `rustls` keeps a
/// single stateful connection, so reads and writes share it. `conn` is
/// the `rustls` enum over client and server connections, so one
/// [`SocketKind::Tls`] serves both a `connect_tls` client and an
/// `accept`ed server socket.
struct TlsState {
    conn: Connection,
    sock: TcpStream,
}

/// How long `connect` / `connect_tls` wait for the TCP handshake before
/// giving up — a bounded default, since neither takes a timeout arg.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Per-syscall read size for the delimiter-scanning helper. Also the
/// reactor's per-event read size — see [`crate::vm::reactor`].
pub(crate) const CHUNK: usize = 8192;
/// Cap on a single `read_chunk` syscall — a huge `n` must not allocate
/// gigabytes up front. A short read is valid; the caller loops.
pub(crate) const MAX_DIRECT_READ: usize = 65536;
/// Cap on a single UDP datagram receive buffer.
const MAX_DATAGRAM: usize = 65536;
/// Poll cadence for the blocking [`SocketInner::accept`] — the wait
/// between retries while no connection is pending. A listener has no
/// `shutdown`, so `close` cannot wake a blocked `accept` directly;
/// instead `accept` polls a non-blocking listener and observes the
/// `closed` flag within this bound. It only sleeps while *idle* —
/// queued connections are returned at once.
///
/// Since sub-phase B2 a `go` coroutine's `accept` is driven on the
/// async-IO reactor ([`crate::vm::reactor`]) — readiness-based, no
/// polling. This blocking form is the inline fallback for an `accept`
/// run with no green threads (or inside a generator).
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
    /// A connected TLS stream (client or server side). Not split —
    /// `rustls` keeps a single stateful connection, so one lock covers
    /// both directions. Boxed to keep the `SocketKind` enum small.
    Tls(Mutex<Box<TlsState>>),
    /// A listening TLS server socket. `accept` performs the TCP accept,
    /// then wraps the stream in a server-side `rustls` connection built
    /// from `config`.
    TlsListener { listener: TcpListener, config: Arc<ServerConfig> },
}

pub struct SocketInner {
    kind: SocketKind,
    /// Set by `close`; every operation checks it and raises `Closed`.
    closed: AtomicBool,
    /// Tracks whether the fd is currently in non-blocking mode. The
    /// reactor sets it non-blocking to drive an op on the poll thread;
    /// the inline blocking executor sets it back. Only meaningful for
    /// reactor-managed kinds (a connected `TcpStream`); a redundant
    /// fcntl is skipped when the mode already matches.
    nonblocking: AtomicBool,
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
        nonblocking: AtomicBool::new(false),
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
        nonblocking: AtomicBool::new(false),
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
        nonblocking: AtomicBool::new(false),
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

/// Build a TLS client config. With no `extra_ca` this is the shared,
/// cached [`tls_config`]. With `extra_ca` (a PEM bundle of additional
/// trusted certificates) a fresh, uncached config is built whose root
/// store holds the OS trust roots *plus* those certificates — used to
/// connect to a private-CA or self-signed service (e.g. tigr's own
/// `listen_tls` server in the test suite).
fn tls_client_config(
    extra_ca: Option<&[u8]>,
) -> Result<Arc<ClientConfig>, NetError> {
    let Some(ca_pem) = extra_ca else {
        return Ok(tls_config());
    };
    let mut roots = RootCertStore::empty();
    let loaded = rustls_native_certs::load_native_certs();
    roots.add_parsable_certificates(loaded.certs);
    let extra: Vec<CertificateDer<'static>> =
        CertificateDer::pem_slice_iter(ca_pem)
            .collect::<Result<_, _>>()
            .map_err(|e| NetError::Tls(format!("trusted CA PEM: {e}")))?;
    if extra.is_empty() {
        return Err(NetError::Tls(
            "trusted CA PEM contained no certificates".into(),
        ));
    }
    for cert in extra {
        roots
            .add(cert)
            .map_err(|e| NetError::Tls(format!("trusted CA: {e}")))?;
    }
    let provider = rustls::crypto::ring::default_provider();
    let config = ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .expect("ring provider supports the default TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// Open a TLS-encrypted TCP connection to `host:port`. `host` doubles
/// as the certificate-verification server name. `extra_ca_pem`, when
/// given, adds trusted root certificates beyond the OS trust store. The
/// handshake runs to completion here (this is offloaded to the worker
/// pool, since DNS and the handshake are blocking); the resulting
/// connection is then driven for steady-state data I/O on the reactor.
pub fn connect_tls(
    host: &str,
    port: u16,
    extra_ca_pem: Option<&[u8]>,
) -> Result<SocketHandle, NetError> {
    // Build the config first — bad CA PEM fails before we touch the net.
    let config = tls_client_config(extra_ca_pem)?;
    let addr = resolve(host, port)?;
    let mut tcp = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
    let shutdown = tcp.try_clone()?;
    let server_name = ServerName::try_from(host.to_string())
        .map_err(|e| NetError::Tls(format!("invalid server name '{host}': {e}")))?;
    let mut conn = ClientConnection::new(config, server_name)
        .map_err(|e| NetError::Tls(e.to_string()))?;
    // Drive the handshake to completion against the blocking socket.
    conn.complete_io(&mut tcp)
        .map_err(|e| NetError::Tls(format!("handshake failed: {e}")))?;
    Ok(Arc::new(SocketInner {
        kind: SocketKind::Tls(Mutex::new(Box::new(TlsState {
            conn: Connection::Client(conn),
            sock: tcp,
        }))),
        closed: AtomicBool::new(false),
        nonblocking: AtomicBool::new(false),
        shutdown: Mutex::new(Some(shutdown)),
        read_buf: Mutex::new(Vec::new()),
        id: next_id(),
    }))
}

/// Build a TLS server config from a PEM certificate chain and a PEM
/// private key. A bad PEM, an empty chain, or a cert / key mismatch all
/// surface as [`NetError::Tls`].
fn tls_server_config(
    cert_pem: &[u8],
    key_pem: &[u8],
) -> Result<Arc<ServerConfig>, NetError> {
    let chain: Vec<CertificateDer<'static>> =
        CertificateDer::pem_slice_iter(cert_pem)
            .collect::<Result<_, _>>()
            .map_err(|e| NetError::Tls(format!("certificate PEM: {e}")))?;
    if chain.is_empty() {
        return Err(NetError::Tls(
            "certificate PEM contained no certificates".into(),
        ));
    }
    let key = PrivateKeyDer::from_pem_slice(key_pem)
        .map_err(|e| NetError::Tls(format!("private key PEM: {e}")))?;
    let provider = rustls::crypto::ring::default_provider();
    let config = ServerConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .expect("ring provider supports the default TLS versions")
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map_err(|e| NetError::Tls(format!("certificate / key: {e}")))?;
    Ok(Arc::new(config))
}

/// Bind a listening TLS server socket. The certificate and key are PEM
/// content (not file paths), keeping `Net` filesystem-free. The config
/// is built *before* binding, so a bad cert / key fails fast.
pub fn listen_tls(
    host: &str,
    port: u16,
    cert_pem: &[u8],
    key_pem: &[u8],
) -> Result<SocketHandle, NetError> {
    let config = tls_server_config(cert_pem, key_pem)?;
    let listener = TcpListener::bind((host, port))?;
    // Non-blocking, exactly like `listen` — see `SocketInner::accept`.
    listener.set_nonblocking(true)?;
    Ok(Arc::new(SocketInner {
        kind: SocketKind::TlsListener { listener, config },
        closed: AtomicBool::new(false),
        nonblocking: AtomicBool::new(false),
        shutdown: Mutex::new(None),
        read_buf: Mutex::new(Vec::new()),
        id: next_id(),
    }))
}

/// Wrap a freshly-accepted `TcpStream` in a server-side TLS socket. The
/// `rustls` handshake is *not* run here — it completes lazily on the
/// first reactor-driven I/O (see [`tls_read`] / [`pump_handshake`]).
fn tls_server_socket(
    stream: TcpStream,
    config: Arc<ServerConfig>,
) -> Result<SocketHandle, NetError> {
    let shutdown = stream.try_clone()?;
    let conn = ServerConnection::new(config)
        .map_err(|e| NetError::Tls(e.to_string()))?;
    Ok(Arc::new(SocketInner {
        kind: SocketKind::Tls(Mutex::new(Box::new(TlsState {
            conn: Connection::Server(conn),
            sock: stream,
        }))),
        closed: AtomicBool::new(false),
        nonblocking: AtomicBool::new(false),
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
                let mut guard = m.lock().unwrap();
                let st: &mut TlsState = &mut guard;
                Ok(tls_read(st, buf)?)
            }
            SocketKind::TcpListener(_)
            | SocketKind::Udp(_)
            | SocketKind::TlsListener { .. } => Err(NetError::WrongKind(
                "read expects a connected stream".into(),
            )),
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
                let mut guard = m.lock().unwrap();
                let st: &mut TlsState = &mut guard;
                let mut off = 0;
                while off < data.len() {
                    off += tls_write(st, &data[off..])?;
                }
                flush_tls(st)?;
                Ok(())
            }
            SocketKind::TcpListener(_)
            | SocketKind::Udp(_)
            | SocketKind::TlsListener { .. } => Err(NetError::WrongKind(
                "write expects a connected stream".into(),
            )),
        }
    }

    /// Accept the next inbound connection on a listener socket.
    /// Block for the next inbound connection. The listener is
    /// non-blocking, so this polls — observing a concurrent `close`
    /// within `ACCEPT_POLL` and raising `Closed` — rather than parking
    /// in a syscall that `close` could not interrupt.
    pub fn accept(&self) -> Result<SocketHandle, NetError> {
        let (listener, config) = match &self.kind {
            SocketKind::TcpListener(l) => (l, None),
            SocketKind::TlsListener { listener, config } => {
                (listener, Some(config))
            }
            _ => {
                return Err(NetError::WrongKind(
                    "accept expects a listener".into(),
                ));
            }
        };
        loop {
            if self.closed.load(Ordering::Acquire) {
                return Err(NetError::Closed);
            }
            match listener.accept() {
                Ok((stream, _addr)) => {
                    // The listener is non-blocking; hand back a blocking
                    // stream (its clones inherit the mode).
                    stream.set_nonblocking(false)?;
                    return match config {
                        Some(cfg) => tls_server_socket(stream, cfg.clone()),
                        None => tcp_stream_socket(stream),
                    };
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(ACCEPT_POLL);
                }
                Err(e) => return Err(e.into()),
            }
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
            SocketKind::TlsListener { listener, .. } => listener.local_addr()?,
        })
    }

    /// The address of the connected peer.
    pub fn peer_addr(&self) -> Result<SocketAddr, NetError> {
        match &self.kind {
            SocketKind::TcpStream { read, .. } => {
                Ok(read.lock().unwrap().peer_addr()?)
            }
            SocketKind::Tls(m) => Ok(m.lock().unwrap().sock.peer_addr()?),
            SocketKind::TcpListener(_)
            | SocketKind::Udp(_)
            | SocketKind::TlsListener { .. } => Err(NetError::WrongKind(
                "peer_addr expects a connected stream".into(),
            )),
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
            SocketKind::TcpListener(_) | SocketKind::TlsListener { .. } => {
                Err(NetError::WrongKind(
                    "set_timeout is not supported on a listener socket".into(),
                ))
            }
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

    // -- reactor support ---------------------------------------------
    //
    // The reactor ([`crate::vm::reactor`]) drives a socket op on its
    // poll thread with the fd in non-blocking mode. These primitives
    // expose just enough of `SocketInner` for the reactor's state
    // machine, and for the inline blocking executor to restore the
    // blocking mode a prior reactor op may have left behind.

    /// Has the socket been `close`d?
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Is this a TLS socket? A TLS op oscillates between needing read
    /// and write readiness as `rustls` drives its records, so the
    /// reactor registers it for both interests at once.
    pub fn is_tls(&self) -> bool {
        matches!(self.kind, SocketKind::Tls(_))
    }

    /// The raw handle to register for a read-direction reactor op — the
    /// read half of a connected stream, the listener handle for
    /// `accept`, the datagram handle for `recv_from`, or the single TCP
    /// handle under a TLS connection.
    pub fn read_raw_handle(&self) -> Option<RawHandle> {
        match &self.kind {
            SocketKind::TcpStream { read, .. } => {
                Some(raw_handle_of(&*read.lock().unwrap()))
            }
            SocketKind::TcpListener(l) => Some(raw_handle_of(l)),
            SocketKind::Udp(u) => Some(raw_handle_of(u)),
            SocketKind::Tls(m) => Some(raw_handle_of(&m.lock().unwrap().sock)),
            SocketKind::TlsListener { listener, .. } => {
                Some(raw_handle_of(listener))
            }
        }
    }

    /// The raw handle to register for a write-direction reactor op. For
    /// a plain stream the write half is a distinct `dup`'d handle, so a
    /// concurrent read and write register two independent handles; a TLS
    /// connection has one handle, registered for both interests.
    pub fn write_raw_handle(&self) -> Option<RawHandle> {
        match &self.kind {
            SocketKind::TcpStream { write, .. } => {
                Some(raw_handle_of(&*write.lock().unwrap()))
            }
            SocketKind::Tls(m) => Some(raw_handle_of(&m.lock().unwrap().sock)),
            _ => None,
        }
    }

    /// Toggle the socket's non-blocking mode. On unix `O_NONBLOCK` lives
    /// on the shared open file description, so one clone's flag is every
    /// clone's; on Windows each duplicated socket handle carries its own
    /// flag, so the read and write halves must both be toggled (the
    /// reactor reads on one and writes on the other). Setting both is a
    /// harmless redundant `fcntl` on unix. A redundant call is skipped
    /// when the mode already matches.
    pub fn set_nonblocking_mode(&self, nb: bool) -> io::Result<()> {
        if self.nonblocking.load(Ordering::Acquire) == nb {
            return Ok(());
        }
        match &self.kind {
            SocketKind::TcpStream { read, write } => {
                read.lock().unwrap().set_nonblocking(nb)?;
                write.lock().unwrap().set_nonblocking(nb)?;
            }
            SocketKind::Tls(m) => {
                m.lock().unwrap().sock.set_nonblocking(nb)?;
            }
            SocketKind::Udp(u) => u.set_nonblocking(nb)?,
            // A listener manages its own non-blocking mode (it is bound
            // permanently non-blocking — see `listen` / `listen_tls`).
            SocketKind::TcpListener(_) | SocketKind::TlsListener { .. } => {
                return Ok(());
            }
        }
        self.nonblocking.store(nb, Ordering::Release);
        Ok(())
    }

    /// Read decrypted plaintext (TLS) or raw bytes (plain stream)
    /// without blocking. `Ok(0)` is end-of-stream; `WouldBlock` means
    /// the reactor should wait for the next readiness event. For a TLS
    /// socket this hand-drives `rustls` — see [`tls_read`].
    pub fn nb_read(&self, buf: &mut [u8]) -> io::Result<usize> {
        match &self.kind {
            SocketKind::TcpStream { read, .. } => {
                let guard = read.lock().unwrap();
                let mut stream: &TcpStream = &guard;
                stream.read(buf)
            }
            SocketKind::Tls(m) => {
                let mut guard = m.lock().unwrap();
                let st: &mut TlsState = &mut guard;
                tls_read(st, buf)
            }
            SocketKind::TcpListener(_)
            | SocketKind::Udp(_)
            | SocketKind::TlsListener { .. } => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "reactor read expects a connected stream",
            )),
        }
    }

    /// Write bytes without blocking, returning the count consumed. For
    /// a TLS socket this buffers plaintext into `rustls` and pushes out
    /// TLS records — see [`tls_write`]; the encrypted tail is drained
    /// by [`nb_flush`].
    pub fn nb_write(&self, buf: &[u8]) -> io::Result<usize> {
        match &self.kind {
            SocketKind::TcpStream { write, .. } => {
                let guard = write.lock().unwrap();
                let mut stream: &TcpStream = &guard;
                stream.write(buf)
            }
            SocketKind::Tls(m) => {
                let mut guard = m.lock().unwrap();
                let st: &mut TlsState = &mut guard;
                tls_write(st, buf)
            }
            SocketKind::TcpListener(_)
            | SocketKind::Udp(_)
            | SocketKind::TlsListener { .. } => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "reactor write expects a connected stream",
            )),
        }
    }

    /// Flush any bytes still buffered below the op's accounting. A
    /// plain socket's `write` already reaches the kernel send buffer,
    /// so this is a no-op for it; a TLS socket may hold encrypted
    /// records `tls_write` could not push out yet. `WouldBlock` means
    /// the reactor should wait for write readiness.
    pub fn nb_flush(&self) -> io::Result<()> {
        match &self.kind {
            SocketKind::Tls(m) => {
                let mut guard = m.lock().unwrap();
                let st: &mut TlsState = &mut guard;
                flush_tls(st)
            }
            _ => Ok(()),
        }
    }

    /// One non-blocking `accept` attempt. The listener is permanently
    /// non-blocking, so `WouldBlock` (surfaced as `NetError::Io`) means
    /// no connection is pending — the reactor waits for the next
    /// readiness event. A successful accept hands back a blocking
    /// stream socket.
    pub fn nb_accept(&self) -> Result<SocketHandle, NetError> {
        match &self.kind {
            SocketKind::TcpListener(l) => {
                let (stream, _addr) = l.accept()?;
                stream.set_nonblocking(false)?;
                tcp_stream_socket(stream)
            }
            SocketKind::TlsListener { listener, config } => {
                let (stream, _addr) = listener.accept()?;
                stream.set_nonblocking(false)?;
                tls_server_socket(stream, config.clone())
            }
            _ => Err(NetError::WrongKind(
                "accept expects a listener".into(),
            )),
        }
    }

    /// Drain up to `n` bytes already buffered by an over-reading
    /// `read_until`; an empty vec means the buffer is empty.
    pub fn take_buffered(&self, n: usize) -> Vec<u8> {
        let mut buf = self.read_buf.lock().unwrap();
        let k = n.min(buf.len());
        buf.drain(..k).collect()
    }

    /// Drain the buffer through the first `delim` byte (inclusive);
    /// `None` when the delimiter is not buffered yet.
    pub fn take_buffered_until(&self, delim: u8) -> Option<Vec<u8>> {
        let mut buf = self.read_buf.lock().unwrap();
        let pos = buf.iter().position(|&b| b == delim)?;
        Some(buf.drain(..=pos).collect())
    }

    /// Drain every buffered byte.
    pub fn take_buffered_all(&self) -> Vec<u8> {
        std::mem::take(&mut *self.read_buf.lock().unwrap())
    }

    /// Append freshly-received bytes to the read buffer.
    pub fn push_buffered(&self, data: &[u8]) {
        self.read_buf.lock().unwrap().extend_from_slice(data);
    }
}

// ---------------------------------------------------------------------
// TLS drive loop
// ---------------------------------------------------------------------
//
// `rustls` is sans-IO: it owns the TLS state machine but performs no
// syscalls. These helpers hand-drive it over the raw `TcpStream`, the
// `tokio-rustls` pattern. They work whether `sock` is blocking (the
// inline executor — a syscall simply blocks) or non-blocking (the
// reactor — a syscall surfaces `WouldBlock` for the op to wait on).
// Because a TLS op may need read *or* write readiness at any moment,
// the reactor registers a TLS fd for both interests and re-drives the
// loop on either; these helpers never have to choose an interest.

/// Push every TLS record `rustls` currently wants to send. On a
/// non-blocking socket a full send buffer surfaces as `WouldBlock`.
fn flush_tls(st: &mut TlsState) -> io::Result<()> {
    while st.conn.wants_write() {
        if st.conn.write_tls(&mut st.sock)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "tls: socket accepted no bytes",
            ));
        }
    }
    Ok(())
}

/// Drive a TLS handshake — both directions — until it completes. A
/// freshly-`accept`ed server connection has not processed the peer's
/// ClientHello yet: a `tls_write` on it would buffer plaintext that
/// `flush_tls` has nothing to push, deadlocking a protocol where the
/// server speaks first. Running this prelude completes the handshake
/// organically. A no-op once the handshake is already done. On a
/// non-blocking socket `WouldBlock` surfaces when neither side can make
/// progress — the reactor parks until the fd is ready again.
fn pump_handshake(st: &mut TlsState) -> io::Result<()> {
    while st.conn.is_handshaking() {
        // Push whatever the handshake currently wants to send.
        flush_tls(st)?;
        if !st.conn.is_handshaking() {
            break;
        }
        if !st.conn.wants_read() {
            // Still handshaking, nothing left to send, yet rustls does
            // not want to read either — the peer has gone away
            // mid-handshake (e.g. a `close_notify`). Surface that as an
            // EOF, never as a `WouldBlock`: a blocking caller would
            // misread `WouldBlock` as a timeout, and it would never make
            // progress. `WouldBlock` here comes only from `read_tls`
            // itself, so `pump_handshake` stays mode-agnostic — like
            // `tls_read` / `tls_write`.
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "tls: peer closed during handshake",
            ));
        }
        match st.conn.read_tls(&mut st.sock) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "tls: peer closed during handshake",
                ));
            }
            Ok(_) => {
                st.conn.process_new_packets().map_err(|e| {
                    io::Error::new(io::ErrorKind::InvalidData, e)
                })?;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Drive `rustls` until it yields decrypted plaintext into `buf`.
/// `Ok(0)` is a clean end-of-stream; an unclean close surfaces as an
/// `io::Error`. `WouldBlock` (non-blocking socket) means the op should
/// park until the fd is ready again.
fn tls_read(st: &mut TlsState, buf: &mut [u8]) -> io::Result<usize> {
    // Complete a still-pending handshake first — defensive; the loop
    // below would also drive it, but a server connection that has never
    // been written to may still be mid-handshake here.
    pump_handshake(st)?;
    loop {
        // Hand back any plaintext rustls has already decrypted.
        match st.conn.reader().read(buf) {
            Ok(n) => return Ok(n),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e),
        }
        // None buffered: push out any records rustls wants to send
        // (handshake continuation, key update, alert), then pull more
        // TLS bytes from the peer and process them.
        flush_tls(st)?;
        if !st.conn.wants_read() {
            return Ok(0);
        }
        match st.conn.read_tls(&mut st.sock) {
            Ok(0) => return Ok(0),
            Ok(_) => {
                st.conn.process_new_packets().map_err(|e| {
                    io::Error::new(io::ErrorKind::InvalidData, e)
                })?;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Buffer plaintext from `buf` into `rustls` and push the resulting TLS
/// records toward the socket. Returns the count of `buf` consumed —
/// possibly partial when `rustls`'s send buffer fills against a slow
/// peer. `Err(WouldBlock)` is returned only when *nothing* was
/// consumed, so a retry never double-sends already-buffered plaintext.
fn tls_write(st: &mut TlsState, buf: &[u8]) -> io::Result<usize> {
    if buf.is_empty() {
        return Ok(0);
    }
    // Complete a pending handshake before buffering any plaintext. This
    // runs before `buf` is touched, so a `WouldBlock` here still means
    // "nothing consumed" — the `WriteAll` retry contract holds.
    pump_handshake(st)?;
    let mut off = 0;
    loop {
        if off < buf.len() {
            match st.conn.writer().write(&buf[off..]) {
                Ok(n) => off += n,
                Err(e) => return if off > 0 { Ok(off) } else { Err(e) },
            }
        }
        // Push records out to free rustls's buffer for more plaintext.
        match flush_tls(st) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                return if off > 0 { Ok(off) } else { Err(e) };
            }
            Err(e) => return if off > 0 { Ok(off) } else { Err(e) },
        }
        if off >= buf.len() {
            return Ok(off);
        }
        // flush_tls drained the send buffer — loop to buffer the rest.
    }
}

// ---------------------------------------------------------------------
// Declarative socket operations
// ---------------------------------------------------------------------

/// A socket operation, described declaratively so the *same* op can run
/// two ways: inline (blocking, on the actor thread, when nothing else
/// is waiting) or on the reactor (a non-blocking state machine). The
/// read accumulators (`got`) and the write cursor (`sent`) let the
/// reactor resume an op across several readiness events; the inline
/// executor starts them fresh. Built by a `Net` `NativeKind::Socket`
/// native; carried in a [`ReactorOp`].
pub enum SocketOp {
    /// `Net.accept(listener)` — the next inbound connection.
    Accept,
    /// `Net.recv_from(sock, n)` — one UDP datagram (up to `n` bytes)
    /// plus its sender's address.
    RecvFrom(usize),
    /// `Net.read(sock, n)` — up to `n` bytes (empty = end-of-stream).
    ReadChunk(usize),
    /// `Net.read_exact(sock, n)` — exactly `need` bytes; `got` holds
    /// what has arrived so far.
    ReadExact { need: usize, got: Vec<u8> },
    /// `Net.read_line(sock)` — one `\n`-terminated line, decoded.
    ReadLine,
    /// `Net.read_until(sock, byte)` — up to and including `delim`.
    ReadUntil(u8),
    /// `Net.read_all(sock)` — every byte until end-of-stream; `got`
    /// accumulates across readiness events.
    ReadAll(Vec<u8>),
    /// `Net.write(sock, bytes)` — every byte of `data`; `sent` is the
    /// count written so far.
    WriteAll { data: Vec<u8>, sent: usize },
}

/// A [`SocketOp`] bound to the socket it runs against, plus a label for
/// error messages. The unit a `NativeKind::Socket` native produces and
/// the reactor (or the inline executor) consumes.
pub struct ReactorOp {
    pub socket: SocketHandle,
    pub op: SocketOp,
    pub label: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Socket` crosses actor threads, so its handle must be `Send`
    /// (and `Sync`, since several actors may hold clones). Compile-time.
    #[test]
    fn socket_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        fn assert_send<T: Send>() {}
        assert_send_sync::<SocketInner>();
        assert_send_sync::<SocketHandle>();
        assert_send_sync::<NetError>();
        // A `ReactorOp` is handed to the reactor thread / worker pool.
        assert_send::<SocketOp>();
        assert_send::<ReactorOp>();
    }
}
