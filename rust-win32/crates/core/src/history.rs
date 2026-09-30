//! Upstream's awake-use least-squares estimator, isolated from wall-clock storage.
use crate::{Precision, Reading};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Discharge {
    pub usage: f64,
    pub samples: Vec<(f64, u8)>,
    pub seen: i64,
    #[serde(skip)]
    last: Option<f64>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Estimator {
    pub devices: BTreeMap<String, Discharge>,
}
impl Estimator {
    /// Reset only the awake-time baseline, preserving the learned discharge rate.
    pub fn pause(&mut self, key: &str) {
        if let Some(d) = self.devices.get_mut(key) {
            d.last = None;
        }
    }
    pub fn pause_all(&mut self) {
        for d in self.devices.values_mut() {
            d.last = None;
        }
    }
    /// Recover valid entries independently; loading never counts time while closed.
    pub fn from_value(value: serde_json::Value, now: i64) -> Self {
        let mut estimator = Self::default();
        if let Some(devices) = value.get("devices").and_then(|v| v.as_object()) {
            for (key, value) in devices {
                if let Ok(d) = serde_json::from_value::<Discharge>(value.clone()) {
                    estimator.devices.insert(key.clone(), d);
                }
            }
        }
        estimator.validate(now);
        estimator
    }
    pub fn validate(&mut self, now: i64) {
        self.devices.retain(|_, d| {
            d.last = None;
            if !d.usage.is_finite()
                || d.usage < 0.0
                || (d.seen > 0 && now.saturating_sub(d.seen) >= 60 * 86400)
            {
                return false;
            }
            if d.samples.iter().any(|(time, level)| {
                !time.is_finite() || *time < 0.0 || *time > d.usage || *level > 100
            }) || d
                .samples
                .windows(2)
                .any(|pair| pair[0].0 > pair[1].0 || pair[0].1 <= pair[1].1)
            {
                return false;
            }
            if d.samples.len() > 400 {
                d.samples.drain(..d.samples.len() - 400);
            }
            true
        });
    }
    pub fn record(&mut self, r: &Reading, monotonic: f64) {
        let Some(level) = r.level.filter(|l| *l <= 100) else {
            self.pause(&r.key);
            return;
        };
        if r.precision == Precision::Coarse || !monotonic.is_finite() {
            self.pause(&r.key);
            return;
        }
        let d = self.devices.entry(r.key.clone()).or_default();
        d.seen = r.timestamp;
        if r.charging == Some(true) {
            *d = Discharge {
                seen: r.timestamp,
                ..Default::default()
            };
            return;
        }
        if !r.online() {
            d.last = None;
            return;
        }
        if let Some(last) = d.last {
            d.usage += (monotonic - last).clamp(0.0, 600.0);
        }
        d.last = Some(monotonic);
        if d.samples
            .last()
            .is_some_and(|(_, previous)| level >= previous.saturating_add(3))
        {
            d.usage = 0.0;
            d.samples.clear();
        }
        if d.samples
            .last()
            .is_none_or(|(_, previous)| level < *previous)
        {
            d.samples.push((d.usage, level));
            if d.samples.len() > 400 {
                d.samples.remove(0);
            }
        }
    }
    pub fn seconds_left(&self, key: &str, level: Option<u8>) -> Option<f64> {
        let level = level.filter(|l| *l <= 100)?;
        let d = self.devices.get(key)?;
        let mut samples = d.samples.clone();
        let &(last_time, last_level) = samples.last()?;
        if d.usage > last_time {
            samples.push((d.usage, last_level));
        }
        if samples.len() < 2
            || samples.last()?.0 - samples.first()?.0 < 1800.0
            || samples.first()?.1.saturating_sub(last_level) < 3
        {
            return None;
        }
        let n = samples.len() as f64;
        let mt = samples.iter().map(|s| s.0).sum::<f64>() / n;
        let ml = samples.iter().map(|s| f64::from(s.1)).sum::<f64>() / n;
        let numerator = samples
            .iter()
            .map(|s| (s.0 - mt) * (f64::from(s.1) - ml))
            .sum::<f64>();
        let denominator = samples.iter().map(|s| (s.0 - mt).powi(2)).sum::<f64>();
        if denominator <= 0.0 {
            return None;
        }
        let rate = -numerator / denominator;
        (rate.is_finite() && rate > 0.0)
            .then(|| f64::from(level) / rate)
            .filter(|s| s.is_finite())
    }
}

pub fn format_left(seconds: f64) -> String {
    let hours = seconds / 3600.0;
    if hours < 1.0 {
        "less than 1 h of use left".into()
    } else if hours < 48.0 {
        format!("about {:.0} h of use left", hours.round())
    } else {
        format!("about {:.0} days of use left", (hours / 24.0).round())
    }
}
