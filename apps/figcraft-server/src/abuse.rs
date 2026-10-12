//! What one address may do how often: new connections, publishing without the key and messages
//! (only on a server open to the public), and reports (always). Each is a token bucket per address
//! ([`figcraft_collab_net::hub::ip_key`]: an IPv6 address counts by its /64). The table keeps at
//! most [`MAX_ADDRESSES`] addresses: when it is full the half used longest ago is forgotten, so a
//! flood of addresses costs bounded memory (and forgotten addresses start with full buckets).

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Instant;

use figcraft_collab_net::net::Bucket;

/// Most addresses remembered.
pub const MAX_ADDRESSES: usize = 10_000;

/// Reports one address may send an hour.
pub const REPORTS_PER_HOUR: f64 = 10.0;

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
}

struct Address {
    connects: Bucket,
    publishes: Bucket,
    messages: Bucket,
    reports: Bucket,
    used: Instant,
}

/// The buckets of the addresses seen lately.
pub struct Abuse {
    cfg: AbuseConfig,
    table: HashMap<IpAddr, Address>,
}

/// `n` every `period` seconds on average, and all `n` at once at most.
fn bucket(n: f64, period: f64) -> Bucket {
    Bucket::new(n / period, n.max(1.0))
}

impl Abuse {
    pub fn new(cfg: AbuseConfig) -> Self {
        Self { cfg, table: HashMap::new() }
    }

    /// Whether `ip` may do `what` now (counting it when it may).
    pub fn allow(&mut self, ip: IpAddr, what: Action, now: Instant) -> bool {
        if !self.cfg.open && what != Action::Report {
            return true;
        }
        if !self.table.contains_key(&ip) && self.table.len() >= MAX_ADDRESSES {
            self.forget_oldest_half();
        }
        let c = self.cfg;
        let a = self.table.entry(ip).or_insert_with(|| Address {
            connects: bucket(f64::from(c.connects_per_minute), 60.0),
            publishes: bucket(f64::from(c.publishes_per_hour), 3600.0),
            // A burst of four seconds' worth (a big paste, a quick drag).
            messages: Bucket::new(f64::from(c.messages_per_second), f64::from(c.messages_per_second) * 4.0),
            reports: bucket(REPORTS_PER_HOUR, 3600.0),
            used: now,
        });
        a.used = now;
        match what {
            Action::Connect => a.connects.take(now),
            Action::Publish => a.publishes.take(now),
            Action::Message => a.messages.take(now),
            Action::Report => a.reports.take(now),
        }
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
