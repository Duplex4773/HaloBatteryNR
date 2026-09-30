use crate::{Connection, Estimator, PollResult, Precision, Reading, Settings};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceView {
    pub reading: Reading,
    pub name: String,
    pub icon: String,
    pub low_alert_at: u8,
    pub seconds_left: Option<u64>,
    pub text: String,
    pub hidden: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub devices: Vec<DeviceView>,
    pub errors: BTreeMap<String, String>,
    pub timestamp: i64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotificationKind {
    Low,
    Full,
}
#[derive(Clone, Debug)]
pub struct Notification {
    pub key: String,
    pub kind: NotificationKind,
    pub title: String,
    pub text: String,
}
#[derive(Default)]
struct AlertState {
    low: bool,
    full: u8,
    threshold: Option<u8>,
}
pub struct Engine {
    pub settings: Settings,
    pub estimator: Estimator,
    by_provider: BTreeMap<String, Vec<Reading>>,
    misses: BTreeMap<String, u8>,
    alerts: BTreeMap<String, AlertState>,
    held: BTreeMap<(String, NotificationKind), Notification>,
    errors: BTreeMap<String, String>,
    suspended: bool,
}
impl Engine {
    pub fn new(settings: Settings, mut estimator: Estimator) -> Self {
        estimator.pause_all();
        Self {
            settings,
            estimator,
            by_provider: BTreeMap::new(),
            misses: BTreeMap::new(),
            alerts: BTreeMap::new(),
            held: BTreeMap::new(),
            errors: BTreeMap::new(),
            suspended: false,
        }
    }
    pub fn suspend(&mut self) {
        self.suspended = true;
        self.estimator.pause_all();
        for r in self.by_provider.values_mut().flatten() {
            r.connection = Connection::Sleeping;
        }
    }
    pub fn resume(&mut self) {
        self.suspended = false;
        self.estimator.pause_all();
        for r in self.by_provider.values_mut().flatten() {
            r.connection = Connection::Stale;
        }
    }
    /// Delivery belongs to the UI. A failed delivery is retried only while relevant.
    pub fn notification_failed(&mut self, notification: Notification) {
        self.held
            .insert((notification.key.clone(), notification.kind), notification);
    }
    pub fn update_settings(&mut self, settings: Settings) {
        self.settings = settings;
        self.by_provider.retain(|provider, readings| {
            let enabled = self.settings.enabled(provider);
            for r in readings {
                if !enabled || self.settings.devices.get(&r.key).is_some_and(|d| d.hidden) {
                    self.estimator.pause(&r.key);
                }
            }
            enabled
        });
        self.errors
            .retain(|provider, _| self.settings.enabled(provider));
        self.clean_states();
    }
    fn clean_states(&mut self) {
        let connected: BTreeSet<_> = self
            .readings()
            .into_iter()
            .filter(|r| !self.settings.devices.get(&r.key).is_some_and(|d| d.hidden))
            .map(|r| r.key)
            .collect();
        self.alerts.retain(|key, _| connected.contains(key));
        let known: BTreeSet<_> = self
            .by_provider
            .values()
            .flatten()
            .map(|r| r.key.clone())
            .collect();
        self.misses.retain(|key, _| known.contains(key));
        self.held.retain(|(key, _), _| connected.contains(key));
    }
    pub fn apply(
        &mut self,
        provider: &str,
        result: PollResult,
        monotonic: f64,
        quiet: bool,
    ) -> Vec<Notification> {
        if self.suspended {
            return Vec::new();
        }
        let quiet = quiet && self.settings.quiet_fullscreen;
        if !self.settings.enabled(provider) {
            if let Some(previous) = self.by_provider.remove(provider) {
                for r in previous {
                    self.estimator.pause(&r.key);
                }
            }
            self.errors.remove(provider);
            self.clean_states();
            return Vec::new();
        }
        let mut fresh = match result {
            Ok(r) => {
                self.errors.remove(provider);
                r
            }
            Err(e) => {
                self.errors.insert(provider.into(), e.to_string());
                if let Some(previous) = self.by_provider.get_mut(provider) {
                    for r in previous {
                        r.connection = Connection::Stale;
                        self.estimator.record(r, monotonic);
                    }
                }
                return Vec::new();
            }
        };
        for r in &mut fresh {
            if r.level.is_some_and(|l| l > 100) {
                r.level = None;
            }
        }
        let seen: BTreeSet<_> = fresh.iter().map(|r| r.key.clone()).collect();
        if let Some(previous) = self.by_provider.get(provider) {
            for r in previous {
                if seen.contains(&r.key) {
                    self.misses.remove(&r.key);
                    continue;
                }
                self.estimator.pause(&r.key);
                let misses = self.misses.entry(r.key.clone()).or_default();
                *misses = misses.saturating_add(1);
                let limit = if ["xinput", "playstation"].contains(&provider) {
                    1
                } else {
                    2
                };
                if *misses < limit {
                    let mut cached = r.clone();
                    cached.connection = Connection::Stale;
                    fresh.push(cached);
                }
            }
        }
        self.by_provider.insert(provider.into(), fresh);
        let readings = self.readings();
        self.clean_states();
        let mut notifications = Vec::new();
        for r in readings {
            if self.settings.devices.get(&r.key).is_some_and(|d| d.hidden) {
                self.estimator.pause(&r.key);
                continue;
            }
            if seen.contains(&r.key)
                && self
                    .by_provider
                    .get(provider)
                    .is_some_and(|own| own.iter().any(|o| o.key == r.key && o.source == r.source))
            {
                self.estimator.record(&r, monotonic);
            }
            let low = self.settings.low_for(&r.key);
            let name = self
                .settings
                .devices
                .get(&r.key)
                .and_then(|d| d.name.clone())
                .unwrap_or(r.name.clone());
            let alert = self.alerts.entry(r.key.clone()).or_default();
            if alert.threshold != Some(low) {
                alert.low = false;
                alert.threshold = Some(low);
            }
            if let Some(level) = r.level.filter(|_| r.online()) {
                if r.charging == Some(true) || level > low.saturating_add(5) {
                    alert.low = false;
                } else if low > 0 && level <= low && !alert.low && self.settings.notify {
                    alert.low = true;
                    notifications.push(Notification {
                        key: r.key.clone(),
                        kind: NotificationKind::Low,
                        title: "Low battery".into(),
                        text: if r.approx.is_some() || r.precision == Precision::Coarse {
                            format!("{name}: battery is low. Time to charge.")
                        } else {
                            format!("{name}: {level}% left. Time to charge.")
                        },
                    });
                }
                if level == 100
                    && alert.full == 1
                    && self.settings.full_alert
                    && self.settings.notify
                {
                    notifications.push(Notification {
                        key: r.key.clone(),
                        kind: NotificationKind::Full,
                        title: "Fully charged".into(),
                        text: format!("{name} is fully charged."),
                    });
                }
                if level == 100 {
                    alert.full = 2;
                } else if r.charging != Some(true) {
                    alert.full = 0;
                } else if !(alert.full == 2 && level >= 95) {
                    alert.full = 1;
                }
            }
        }
        for n in notifications {
            self.held.insert((n.key.clone(), n.kind), n);
        }
        if quiet { Vec::new() } else { self.flush_held() }
    }
    pub fn flush_held(&mut self) -> Vec<Notification> {
        if self.suspended {
            return Vec::new();
        }
        let readings = self.readings();
        let mut ready = Vec::new();
        for mut n in std::mem::take(&mut self.held).into_values() {
            if !self.settings.notify
                || (n.kind == NotificationKind::Full && !self.settings.full_alert)
                || self.settings.devices.get(&n.key).is_some_and(|d| d.hidden)
            {
                continue;
            }
            let Some(r) = readings.iter().find(|r| r.key == n.key) else {
                continue;
            };
            if n.kind == NotificationKind::Low
                && (self.settings.low_for(&r.key) == 0
                    || r.charging == Some(true)
                    || r.level.is_some_and(|v| v > self.settings.low_for(&r.key)))
            {
                continue;
            }
            // An unavailable reading cannot confirm recovery or disconnect. Keep the
            // pending alert until fresh data permits delivery or invalidates it.
            if !r.online() || (n.kind == NotificationKind::Low && r.level.is_none()) {
                self.held.insert((n.key.clone(), n.kind), n);
                continue;
            }
            let name = self
                .settings
                .devices
                .get(&r.key)
                .and_then(|d| d.name.as_deref())
                .unwrap_or(&r.name);
            n.text = if n.kind == NotificationKind::Full {
                format!("{name} is fully charged.")
            } else if r.precision == Precision::Coarse || r.approx.is_some() {
                format!("{name}: battery is low. Time to charge.")
            } else {
                format!("{name}: {}% left. Time to charge.", r.level.unwrap())
            };
            ready.push(n);
        }
        ready
    }
    pub fn readings(&self) -> Vec<Reading> {
        let all: Vec<_> = self
            .by_provider
            .iter()
            .filter(|(p, _)| self.settings.enabled(p))
            .flat_map(|(_, r)| r.iter().cloned())
            .collect();
        let mut kept: Vec<Reading> = Vec::new();
        for r in &all {
            let identity = |a: &Reading, b: &Reading| {
                (a.container
                    .as_deref()
                    .and_then(trusted_identity)
                    .is_some_and(|id| {
                        b.container.as_deref().and_then(trusted_identity).as_ref() == Some(&id)
                    }))
                    || (a
                        .serial
                        .as_deref()
                        .and_then(trusted_identity)
                        .is_some_and(|id| {
                            b.serial.as_deref().and_then(trusted_identity).as_ref() == Some(&id)
                        }))
            };
            if r.source == "bluetooth"
                && all.iter().any(|o| {
                    o.source != "bluetooth"
                        && o.source != "xinput"
                        && o.online()
                        && o.level.is_some()
                        && identity(r, o)
                })
            {
                continue;
            }
            if r.source == "xinput"
                && r.via == "bluetooth"
                && all.iter().any(|o| {
                    o.source == "bluetooth"
                        && o.kind == "gamepad"
                        && o.online()
                        && o.level.is_some()
                        && identity(r, o)
                })
            {
                continue;
            }
            if let Some(i) = kept.iter().position(|o| o.key == r.key) {
                if r.online()
                    && (!kept[i].online()
                        || (r.level.is_some() && (kept[i].level.is_none() || r.via == "usb")))
                {
                    kept[i] = r.clone();
                }
            } else {
                kept.push(r.clone());
            }
        }
        kept.sort_by(|a, b| a.key.cmp(&b.key));
        kept
    }
    pub fn snapshot(&self, timestamp: i64) -> Snapshot {
        Snapshot {
            timestamp,
            errors: self.errors.clone(),
            devices: self
                .readings()
                .into_iter()
                .map(|r| {
                    let preferences = self
                        .settings
                        .devices
                        .get(&r.key)
                        .cloned()
                        .unwrap_or_default();
                    let name = preferences.name.unwrap_or_else(|| r.name.clone());
                    let icon = preferences.icon.unwrap_or_else(|| {
                        if r.kind.is_empty() {
                            "bluetooth".into()
                        } else {
                            r.kind.clone()
                        }
                    });
                    let seconds_left = if self.settings.time_left
                        && r.online()
                        && r.charging != Some(true)
                        && r.precision != Precision::Coarse
                        && !preferences.hidden
                    {
                        self.estimator
                            .seconds_left(&r.key, r.level)
                            .map(|s| s as u64)
                    } else {
                        None
                    };
                    let mut text = format!(
                        "{name}: {}",
                        r.approx.clone().unwrap_or_else(|| r
                            .level
                            .map_or("battery unknown".into(), |l| format!("{l}%")))
                    );
                    if r.charging == Some(true) {
                        text.push_str(" · charging");
                    }
                    if !r.online() {
                        text.push_str(if r.connection == Connection::Sleeping {
                            " · asleep"
                        } else {
                            " · reading unavailable"
                        });
                    }
                    if let Some(s) = seconds_left {
                        text.push_str(" · ");
                        text.push_str(&crate::history::format_left(s as f64));
                    }
                    DeviceView {
                        low_alert_at: self.settings.low_for(&r.key),
                        reading: r,
                        name,
                        icon,
                        seconds_left,
                        text,
                        hidden: preferences.hidden,
                    }
                })
                .collect(),
        }
    }
}

fn trusted_identity(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_uppercase();
    if normalized.is_empty()
        || matches!(normalized.as_str(), "UNKNOWN" | "NONE" | "N/A" | "NULL")
        || normalized
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .all(|c| c == '0')
    {
        None
    } else {
        Some(normalized)
    }
}
