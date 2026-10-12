//! The WebSocket plumbing both ends share: one connection read by one thread and written by
//! another, limits, and rate limiting.
//!
//! Each connection has a reader thread blocked in `read` and a writer thread blocked on its
//! queue, so an idle session costs no CPU. Both sides of a connection are tungstenite state
//! machines over the same socket; every write goes through one lock and writes whole frames, so
//! frames from the two never interleave.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

pub use crate::limits::NetLimits;

use tungstenite::protocol::WebSocketConfig;

use crate::invite::Endpoint;
use crate::tls::{self, Tls};

/// tungstenite's settings for messages up to `max` bytes.
pub fn config(max: usize) -> WebSocketConfig {
    WebSocketConfig::default().max_message_size(Some(max)).max_frame_size(Some(max))
}

/// One side (read or write) of a connection: reads from its own handle, writes whole buffers
/// under the connection's write lock; through the connection's TLS session when it has one.
pub struct Duplex {
    read: TcpStream,
    write: Arc<Mutex<TcpStream>>,
    tls: Option<Tls>,
    /// What the bytes read count against (a server's reader), if anything.
    meter: Option<Meter>,
}

impl Duplex {
    /// Two handles on `stream` (for the reader and the writer) and one to shut it down with.
    pub fn split(stream: TcpStream, tls: Option<Tls>) -> io::Result<(Duplex, Duplex, TcpStream)> {
        let write = Arc::new(Mutex::new(stream.try_clone()?));
        let a = Duplex { read: stream.try_clone()?, write: write.clone(), tls: tls.clone(), meter: None };
        let b = Duplex { read: stream.try_clone()?, write, tls, meter: None };
        Ok((a, b, stream))
    }

    /// Count what this handle reads against `meter`.
    pub fn meter(&mut self, meter: Meter) {
        self.meter = Some(meter);
    }

    /// What was read so far became a message (handed on): it no longer counts as being read.
    pub fn read_done(&mut self) {
        if let Some(m) = self.meter.as_mut() {
            m.release();
        }
    }
}

/// Bytes being read into messages (not whole yet), per connection, per block of addresses and
/// for every connection together: a read that takes a block over its share, or takes everyone
/// over the budget while its block holds a large part of it, fails, so the biggest readers are
/// dropped and memory for half-read messages stays bounded.
pub struct Meter {
    mine: usize,
    block: Arc<AtomicUsize>,
    all: Arc<AtomicUsize>,
    max_block: usize,
    max_all: usize,
}

impl Meter {
    pub fn new(block: Arc<AtomicUsize>, all: Arc<AtomicUsize>, max_block: usize, max_all: usize) -> Self {
        Self { mine: 0, block, all, max_block, max_all }
    }

    fn add(&mut self, n: usize) -> io::Result<()> {
        self.mine += n;
        let block = self.block.fetch_add(n, Ordering::Relaxed).saturating_add(n);
        let all = self.all.fetch_add(n, Ordering::Relaxed).saturating_add(n);
        // Over its block's share; or over everyone's budget while its block holds a quarter of it
        // (the big readers go first); or far over everyone's budget.
        if block > self.max_block || (all > self.max_all && block.saturating_mul(4) >= self.max_all) || all > self.max_all.saturating_mul(2) {
            return Err(io::Error::other("the server is busy reading from others: try again in a moment"));
        }
        Ok(())
    }

    fn release(&mut self) {
        self.block.fetch_sub(self.mine, Ordering::Relaxed);
        self.all.fetch_sub(self.mine, Ordering::Relaxed);
        self.mine = 0;
    }
}

impl Drop for Meter {
    fn drop(&mut self) {
        self.release();
    }
}

impl Duplex {
    /// How long a read on this handle may block (socket options are per handle: set them on the
    /// one that reads).
    pub fn set_read_timeout(&self, d: Option<Duration>) -> io::Result<()> {
        self.read.set_read_timeout(d)
    }
}

impl Read for Duplex {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = match &self.tls {
            Some(t) => tls::read(t, &mut self.read, &self.write, buf),
            None => self.read.read(buf),
        }?;
        if let Some(m) = self.meter.as_mut() {
            m.add(n)?;
        }
        Ok(n)
    }
}

impl Write for Duplex {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(t) = &self.tls {
            return tls::write(t, &self.write, buf);
        }
        self.write.lock().unwrap_or_else(PoisonError::into_inner).write_all(buf)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.write.lock().unwrap_or_else(PoisonError::into_inner).flush()
    }
}

/// Close the socket both ways (wakes a reader blocked on it).
pub fn shutdown(s: &TcpStream) {
    let _ = s.shutdown(Shutdown::Both);
}

/// Connect to `server` (through TLS for `wss://`): the reader's and the writer's handles, and the
/// socket to shut it down with.
pub fn open(server: &Endpoint, timeout: Duration, limits: &NetLimits) -> Result<(Duplex, Duplex, TcpStream), String> {
    let stream = dial(&server.host, timeout, limits)?;
    let tls = if server.secure { Some(tls::handshake(&stream, &server.host)?) } else { None };
    Duplex::split(stream, tls).map_err(|e| e.to_string())
}

/// Connect to `host` (`name:port`), trying each of its addresses within `timeout`, with Nagle off
/// and reads and writes bounded by `limits`' hello and write timeouts.
fn dial(host: &str, timeout: Duration, limits: &NetLimits) -> Result<TcpStream, String> {
    use std::net::ToSocketAddrs;
    let addrs = host.to_socket_addrs().map_err(|e| format!("can't find {host}: {e}"))?;
    let mut last = format!("can't find {host}");
    for a in addrs {
        match TcpStream::connect_timeout(&a, timeout) {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                s.set_read_timeout(Some(limits.hello_timeout)).map_err(|e| e.to_string())?;
                s.set_write_timeout(Some(limits.write_timeout)).map_err(|e| e.to_string())?;
                return Ok(s);
            }
            Err(e) => last = format!("no answer from {a}: {e}"),
        }
    }
    Err(last)
}

/// A token bucket: `rate` per second on average, `burst` at once.
#[derive(Debug)]
pub struct Bucket {
    rate: f64,
    burst: f64,
    tokens: f64,
    at: Instant,
}

impl Bucket {
    pub fn new(rate: f64, burst: f64) -> Self {
        Self { rate, burst, tokens: burst, at: Instant::now() }
    }

    /// Spend one token at `now`; false when there are none left.
    pub fn take(&mut self, now: Instant) -> bool {
        self.take_n(now, 1.0)
    }

    /// Spend `n` tokens at `now`; false (spending none) when there aren't that many.
    pub fn take_n(&mut self, now: Instant, n: f64) -> bool {
        let dt = now.saturating_duration_since(self.at).as_secs_f64();
        self.at = now;
        self.tokens = (self.tokens + dt * self.rate).min(self.burst);
        if self.tokens < n {
            return false;
        }
        self.tokens -= n;
        true
    }

    /// Give back `n` tokens (up to the burst).
    pub fn give(&mut self, n: f64) {
        self.tokens = (self.tokens + n).min(self.burst);
    }
}
