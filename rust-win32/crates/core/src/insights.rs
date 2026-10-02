//! Bounded, conservative summaries of a single device's retained raw history.
use crate::{PollingRate, Precision, Reading};
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct UsageObservation {
    pub reading: Reading,
    pub polling_rate: Option<PollingRate>,
    pub session: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InsightConfidence {
    #[default]
    Insufficient,
    Low,
    Moderate,
}

#[derive(Clone, Debug, Default)]
pub struct RateInsight {
    pub hz: u32,
    pub awake_seconds: u64,
    pub consumed_percent: u64,
    /// Distinct endpoints of accepted intervals.
    pub sample_count: u64,
    /// Accepted intervals reaching a new low within their uninterrupted segment.
    pub drop_count: u64,
    pub confidence: InsightConfidence,
    pub projected_full_charge_hours: Option<f64>,
    pub remaining_hours: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CycleEvidence {
    /// Discharge started after an explicitly observed charging reading.
    /// This does not imply that the battery reached 100%.
    ObservedCharge,
    InferredCharge,
    /// Only part of an unknown charge cycle was observed.
    Partial,
}

#[derive(Clone, Debug)]
pub struct ChargeCycle {
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub awake_seconds: u64,
    pub start_percent: u8,
    pub end_percent: u8,
    pub consumed_percent: u64,
    pub evidence: CycleEvidence,
    pub current: bool,
}

#[derive(Clone, Debug, Default)]
pub struct BatteryInsights {
    /// Only rates with accepted observed usage, in ascending Hz order.
    pub rates: Vec<RateInsight>,
    /// At most ten recent observed discharge periods, oldest first.
    pub cycles: Vec<ChargeCycle>,
}

pub struct InsightsBuilder {
    previous: Option<UsageObservation>,
    rates: [RateInsight; 7],
    previous_counted_rate: Option<u32>,
    cycles: VecDeque<ChargeCycle>,
    pending_charge: Option<CycleEvidence>,
    segment_low: Option<u8>,
}

impl Default for InsightsBuilder {
    fn default() -> Self {
        Self {
            previous: None,
            rates: [125, 250, 500, 1000, 2000, 4000, 8000].map(|hz| RateInsight {
                hz,
                ..RateInsight::default()
            }),
            previous_counted_rate: None,
            cycles: VecDeque::new(),
            pending_charge: None,
            segment_low: None,
        }
    }
}

fn valid_level(reading: &Reading) -> Option<u8> {
    reading.level.filter(|level| *level <= 100).filter(|_| {
        reading.online() && reading.precision == Precision::Exact && reading.timestamp >= 0
    })
}

fn discharging(reading: &Reading) -> bool {
    valid_level(reading).is_some() && reading.charging == Some(false) && !reading.charging_inferred
}

impl InsightsBuilder {
    /// Use when a corrupt or missing raw row prevents proving adjacency.
    pub fn break_continuity(&mut self) {
        self.previous = None;
        self.previous_counted_rate = None;
        self.pending_charge = None;
        self.segment_low = None;
        self.close_cycle();
    }

    fn close_cycle(&mut self) {
        if let Some(cycle) = self.cycles.back_mut() {
            cycle.current = false;
        }
    }

    fn start_cycle(&mut self, observation: &UsageObservation) {
        if self.cycles.len() == 10 {
            self.cycles.pop_front();
        }
        let reading = &observation.reading;
        let level = valid_level(reading).expect("validated discharge");
        self.cycles.push_back(ChargeCycle {
            start_timestamp: reading.timestamp,
            end_timestamp: reading.timestamp,
            awake_seconds: 0,
            start_percent: level,
            end_percent: level,
            consumed_percent: 0,
            evidence: self.pending_charge.take().unwrap_or(CycleEvidence::Partial),
            current: true,
        });
    }

    /// Feed chronological raw observations for one device. All memory is bounded.
    /// Equal/backwards timestamps and identity changes start a partial period.
    /// Session/rate changes and pauses exclude their boundary interval only.
    pub fn push(&mut self, observation: UsageObservation) {
        let reading = &observation.reading;
        let discontinuity = self.previous.as_ref().is_some_and(|previous| {
            reading.timestamp <= previous.reading.timestamp || reading.key != previous.reading.key
        });
        if discontinuity {
            self.break_continuity();
        }

        if valid_level(reading).is_some() && reading.charging == Some(true) {
            self.close_cycle();
            self.segment_low = None;
            self.pending_charge = Some(if reading.charging_inferred {
                CycleEvidence::InferredCharge
            } else {
                CycleEvidence::ObservedCharge
            });
        } else if discharging(reading) {
            let previous_observation = self.previous.take();
            let previous = previous_observation.as_ref();
            let delta = previous.and_then(|p| reading.timestamp.checked_sub(p.reading.timestamp));
            let continuous = previous.is_some_and(|p| {
                discharging(&p.reading)
                    && delta.is_some_and(|seconds| (1..=600).contains(&seconds))
                    && reading.via == p.reading.via
                    && observation.polling_rate == p.polling_rate
                    && observation.session == p.session
            });
            let level = reading.level.unwrap();
            let increase = self.pending_charge.is_none()
                && ((continuous
                    && self
                        .segment_low
                        .is_some_and(|low| level.saturating_sub(low) >= 3))
                    || self.cycles.back().is_some_and(|cycle| {
                        cycle.current && level.saturating_sub(cycle.end_percent) >= 3
                    }));
            if increase {
                self.close_cycle();
                self.pending_charge = Some(CycleEvidence::InferredCharge);
            }
            if !self.cycles.back().is_some_and(|cycle| cycle.current) {
                self.start_cycle(&observation);
            }
            if continuous && !increase {
                let previous = previous.expect("accepted interval has previous");
                let seconds = delta.expect("accepted positive duration") as u64;
                // Repeated small rebounds cannot manufacture additional drain.
                let low = self.segment_low.unwrap_or(previous.reading.level.unwrap());
                let drop = u64::from(low.saturating_sub(level));
                self.segment_low = Some(low.min(level));
                let cycle = self.cycles.back_mut().expect("discharge cycle");
                cycle.awake_seconds = cycle.awake_seconds.saturating_add(seconds);
                cycle.consumed_percent = cycle.consumed_percent.saturating_add(drop);
                cycle.end_timestamp = reading.timestamp;
                cycle.end_percent = reading.level.unwrap();
                if let Some(polling_rate) = observation.polling_rate.filter(|_| {
                    observation.session.is_some()
                        && observation.polling_rate == previous.polling_rate
                }) {
                    let hz = polling_rate.hz();
                    let rate = self.rates.iter_mut().find(|rate| rate.hz == hz).unwrap();
                    rate.awake_seconds = rate.awake_seconds.saturating_add(seconds);
                    rate.consumed_percent = rate.consumed_percent.saturating_add(drop);
                    rate.sample_count = rate.sample_count.saturating_add(
                        if self.previous_counted_rate == Some(hz) {
                            1
                        } else {
                            2
                        },
                    );
                    rate.drop_count = rate.drop_count.saturating_add(u64::from(drop > 0));
                    self.previous_counted_rate = Some(hz);
                } else {
                    self.previous_counted_rate = None;
                }
            } else {
                self.segment_low = Some(level);
                self.previous_counted_rate = None;
                let cycle = self.cycles.back_mut().unwrap();
                cycle.end_timestamp = reading.timestamp;
                cycle.end_percent = reading.level.unwrap();
            }
        } else {
            // Unknown charging, coarse data or a pause never bridges a drain interval.
            self.segment_low = None;
        }
        if !discharging(reading) {
            self.previous_counted_rate = None;
        }
        self.previous = Some(observation);
    }

    pub fn finish(mut self) -> BatteryInsights {
        for rate in &mut self.rates {
            if rate.awake_seconds >= 1800 && rate.consumed_percent >= 3 {
                rate.confidence = if rate.awake_seconds >= 7200
                    && rate.consumed_percent >= 10
                    && rate.drop_count >= 3
                {
                    InsightConfidence::Moderate
                } else {
                    InsightConfidence::Low
                };
                let hours = rate.awake_seconds as f64 / 36.0 / rate.consumed_percent as f64;
                rate.projected_full_charge_hours = Some(hours);
                if let Some(current) = self.previous.as_ref().filter(|current| {
                    discharging(&current.reading)
                        && current.session.is_some()
                        && current.polling_rate.is_some_and(|r| r.hz() == rate.hz)
                }) {
                    rate.remaining_hours =
                        Some(hours * f64::from(current.reading.level.unwrap()) / 100.0);
                }
            }
        }
        BatteryInsights {
            rates: self
                .rates
                .into_iter()
                .filter(|rate| rate.awake_seconds > 0)
                .collect(),
            cycles: self.cycles.into_iter().collect(),
        }
    }
}
