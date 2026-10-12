//! What one address may do how often: new connections, publishing without the key and messages
//! (only on a server open to the public), and reports and downloads (always). Each is a token
//! bucket per block an address counts in ([`ip_blocks`]: an IPv4 address, or an IPv6 /64 and its
//! /56 and /48 with 4 and 16 times the rate), so a tunnel's /48 doesn't get 65,536 /64s' worth. Publishing has a budget for
//! everyone besides ([`PUBLISHES_ALL`] times one address's). The table keeps at most
//! [`MAX_ADDRESSES`] addresses: when it is full the half used longest ago is forgotten, so a flood
//! of addresses costs bounded memory (and forgotten addresses start with full buckets).

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Instant;

use figcraft_collab_net::hub::{Block, ip_blocks};
use figcraft_collab_net::net::Bucket;

/// Most addresses remembered.
pub const MAX_ADDRESSES: usize = 10_000;

/// Reports one address may send an hour.
pub const REPORTS_PER_HOUR: f64 = 10.0;

/// Bytes one address may download a second (team libraries, plugins), and at once.
pub const DOWNLOAD_RATE: f64 = (4u64 << 20) as f64;
pub const DOWNLOAD_BURST: f64 = (64u64 << 20) as f64;

/// Everyone together may publish without the key this many times what one address may.
pub const PUBLISHES_ALL: f64 = 20.0;

/// The rates (see the server's settings).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AbuseConfig {
    /// Open to the public: connections, publishing and messages are limited.
    pub open: bool,
    pub connects_per_minute: u32,
    pub publishes_per_hour: u32,
    pub messages_per_second: u32,
}

/// What an address does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Connect,
    Publish,
    Message,
    Report,
    /// Downloading this many bytes.
    Download(usize),
}

struct Address {
    connects: Bucket,
    publishes: Bucket,
    messages: Bucket,
    reports: Bucket,
    downloads: Bucket,
    used: Instant,
}

/// The buckets of the addresses seen lately.
pub struct Abuse {
    cfg: AbuseConfig,
    table: HashMap<Block, Address>,
    /// Publishing without the key, everyone together.
    publishes: Bucket,
}

/// `n` every `period` seconds on average, and all `n` at once at most.
fn bucket(n: f64, period: f64) -> Bucket {
    Bucket::new(n / period, n.max(1.0))
}

impl Abuse {
    pub fn new(cfg: AbuseConfig) -> Self {
        let publishes = bucket(f64::from(cfg.publishes_per_hour) * PUBLISHES_ALL, 3600.0);
        Self { cfg, table: HashMap::new(), publishes }
    }

    /// Whether `ip` may do `what` now (counting it, in every block it is in, when it may).
    pub fn allow(&mut self, ip: IpAddr, what: Action, now: Instant) -> bool {
        if !self.cfg.open && matches!(what, Action::Connect | Action::Publish | Action::Message) {
            return true;
        }
        let keys: Vec<(Block, f64)> = ip_blocks(ip).into_iter().map(|(k, times)| (k, f64::from(times))).collect();
        // Every block must have room before any is charged.
        if !keys.iter().all(|&(k, times)| self.room(k, times, what, now)) {
            return false;
        }
        if what == Action::Publish && !self.publishes.take(now) {
            return false;
        }
        for &(k, times) in &keys {
            let (b, n) = Self::pick(self.address(k, times, now), what);
            b.take_n(now, n);
        }
        true
    }

    /// The address's buckets, made when it is new (forgetting the oldest half when full).
    fn address(&mut self, ip: Block, times: f64, now: Instant) -> &mut Address {
        if !self.table.contains_key(&ip) && self.table.len() >= MAX_ADDRESSES {
            self.forget_oldest_half();
        }
        let c = self.cfg;
        let a = self.table.entry(ip).or_insert_with(|| Address {
            connects: bucket(f64::from(c.connects_per_minute) * times, 60.0),
            publishes: bucket(f64::from(c.publishes_per_hour) * times, 3600.0),
            // A burst of four seconds' worth (a big paste, a quick drag).
            messages: Bucket::new(f64::from(c.messages_per_second) * times, f64::from(c.messages_per_second) * times * 4.0),
            reports: bucket(REPORTS_PER_HOUR * times, 3600.0),
            downloads: Bucket::new(DOWNLOAD_RATE * times, DOWNLOAD_BURST * times),
            used: now,
        });
        a.used = now;
        a
    }

    fn pick(a: &mut Address, what: Action) -> (&mut Bucket, f64) {
        match what {
            Action::Connect => (&mut a.connects, 1.0),
            Action::Publish => (&mut a.publishes, 1.0),
            Action::Message => (&mut a.messages, 1.0),
            Action::Report => (&mut a.reports, 1.0),
            Action::Download(n) => (&mut a.downloads, n as f64),
        }
    }

    /// The block has `what`'s tokens now (nothing is spent).
    fn room(&mut self, ip: Block, times: f64, what: Action, now: Instant) -> bool {
        let (b, n) = Self::pick(self.address(ip, times, now), what);
        if b.take_n(now, n) {
            b.give(n);
            return true;
        }
        false
    }

    /// Addresses remembered.
    pub fn len(&self) -> usize {
        self.table.len()
    }

    /// None remembered.
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    fn forget_oldest_half(&mut self) {
        let mut used: Vec<Instant> = self.table.values().map(|a| a.used).collect();
        used.sort_unstable();
        let Some(&cut) = used.get(used.len() / 2) else { return };
        self.table.retain(|_, a| a.used > cut);
    }
}
