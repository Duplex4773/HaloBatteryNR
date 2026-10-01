use hb_core::*;

fn point(timestamp: i64, level: u8) -> UsageObservation {
    let mut reading = Reading::new("device", "Device", "test", timestamp);
    reading.level = Some(level);
    reading.charging = Some(false);
    reading.via = "receiver".into();
    UsageObservation {
        reading,
        polling_rate: Some(PollingRate::try_from(1000).unwrap()),
        session: Some(1),
    }
}

fn summarize(points: impl IntoIterator<Item = UsageObservation>) -> BatteryInsights {
    let mut builder = InsightsBuilder::default();
    for point in points {
        builder.push(point);
    }
    builder.finish()
}

#[test]
fn projections_require_counted_time_and_measured_drop() {
    let summary = summarize((0..=3).map(|i| point(i * 600, 80 - i as u8)));
    let rate = &summary.rates[0];
    assert_eq!(rate.awake_seconds, 1800);
    assert_eq!(rate.consumed_percent, 3);
    assert_eq!(rate.sample_count, 4);
    assert_eq!(rate.drop_count, 3);
    assert_eq!(rate.confidence, InsightConfidence::Low);
    assert!((rate.projected_full_charge_hours.unwrap() - 100.0 / 6.0).abs() < 0.001);
    assert!((rate.remaining_hours.unwrap() - 77.0 / 6.0).abs() < 0.001);
    assert!(
        summarize([point(0, 80), point(600, 70)]).rates[0]
            .projected_full_charge_hours
            .is_none()
    );
    assert!(
        summarize((0..=3).map(|i| point(i * 600, 80))).rates[0]
            .projected_full_charge_hours
            .is_none()
    );
}

#[test]
fn pauses_unknown_coarse_malformed_and_long_gaps_do_not_bridge() {
    for kind in 0..6 {
        let mut middle = point(300, 75);
        match kind {
            0 => middle.reading.connection = Connection::Sleeping,
            1 => middle.reading.connection = Connection::Stale,
            2 => middle.reading.precision = Precision::Coarse,
            3 => middle.reading.charging = None,
            4 => middle.reading.level = Some(101),
            _ => middle.reading.charging_inferred = true,
        }
        let summary = summarize([point(0, 80), middle, point(600, 70), point(900, 69)]);
        assert_eq!(summary.rates[0].awake_seconds, 300);
        assert_eq!(summary.rates[0].consumed_percent, 1);
        assert_eq!(summary.cycles[0].consumed_percent, 1);
    }
    let summary = summarize([point(0, 80), point(601, 70), point(901, 69)]);
    assert_eq!(summary.rates[0].consumed_percent, 1);
}

#[test]
fn switches_and_restart_exclude_boundary_interval() {
    for kind in 0..3 {
        let mut switched = point(300, 75);
        match kind {
            0 => switched.polling_rate = Some(PollingRate::try_from(500).unwrap()),
            1 => switched.session = Some(2),
            _ => switched.reading.via = "bluetooth".into(),
        }
        let mut last = switched.clone();
        last.reading.timestamp = 600;
        last.reading.level = Some(74);
        let summary = summarize([point(0, 80), switched, last]);
        assert_eq!(summary.rates[0].consumed_percent, 1);
        assert_eq!(
            summary
                .cycles
                .iter()
                .map(|c| c.consumed_percent)
                .sum::<u64>(),
            1
        );
    }
}

#[test]
fn old_history_summarizes_cycles_without_learning_rates() {
    let summary = summarize((0..3).map(|i| {
        let mut value = point(i * 300, 80 - i as u8);
        value.polling_rate = None;
        value.session = None;
        value
    }));
    assert!(summary.rates.is_empty());
    assert_eq!(summary.cycles[0].consumed_percent, 2);
    assert_eq!(summary.cycles[0].evidence, CycleEvidence::Partial);
}

#[test]
fn observed_charge_and_level_increase_split_discharge() {
    let mut charging = point(300, 85);
    charging.reading.charging = Some(true);
    let summary = summarize([
        point(0, 80),
        charging,
        point(600, 90),
        point(900, 89),
        point(1200, 95),
        point(1500, 94),
    ]);
    assert_eq!(summary.cycles.len(), 3);
    assert_eq!(summary.cycles[1].evidence, CycleEvidence::ObservedCharge);
    assert_eq!(summary.cycles[2].evidence, CycleEvidence::InferredCharge);
    assert_eq!(summary.cycles[1].consumed_percent, 1);
    assert!(!summary.cycles[1].current);
    assert!(summary.cycles[2].current);
}

#[test]
fn backward_duplicate_corrupt_and_identity_breaks_are_conservative() {
    for timestamp in [0, -1, i64::MIN] {
        let summary = summarize([point(0, 80), point(timestamp, 70), point(300, 69)]);
        assert!(summary.rates.iter().all(|rate| rate.consumed_percent <= 1));
    }
    let mut builder = InsightsBuilder::default();
    builder.push(point(0, 80));
    builder.break_continuity();
    builder.push(point(300, 70));
    assert!(builder.finish().rates.is_empty());
    let mut other = point(300, 70);
    other.reading.key = "other".into();
    assert!(summarize([point(0, 80), other]).rates.is_empty());
}

#[test]
fn cycles_and_rate_buckets_remain_bounded() {
    let summary = summarize((0..100).map(|i| point(i * 300, if i % 2 == 0 { 80 } else { 75 })));
    assert_eq!(summary.cycles.len(), 10);
    assert!(summary.rates.len() <= 7);
    assert_eq!(
        summary.cycles.iter().filter(|cycle| cycle.current).count(),
        1
    );
}

#[test]
fn observed_charge_survives_sleep_unknown_state_and_session_changes() {
    let mut charging = point(0, 80);
    charging.reading.charging = Some(true);
    let mut sleeping = point(300, 90);
    sleeping.reading.connection = Connection::Sleeping;
    sleeping.reading.charging = None;
    let mut awake = point(600, 90);
    awake.session = Some(2);
    let mut last = awake.clone();
    last.reading.timestamp = 900;
    last.reading.level = Some(89);
    let summary = summarize([charging, sleeping, awake, last]);
    assert_eq!(summary.cycles.len(), 1);
    assert_eq!(summary.cycles[0].evidence, CycleEvidence::ObservedCharge);
    assert_eq!(summary.cycles[0].awake_seconds, 300);
    assert_eq!(summary.cycles[0].consumed_percent, 1);
}

#[test]
fn physical_cycle_survives_session_and_rate_changes_without_boundary_drain() {
    let mut charging = point(0, 80);
    charging.reading.charging = Some(true);
    let mut switched = point(900, 70);
    switched.session = Some(2);
    switched.polling_rate = Some(PollingRate::try_from(500).unwrap());
    let mut last = switched.clone();
    last.reading.timestamp = 1200;
    last.reading.level = Some(69);
    let summary = summarize([charging, point(300, 90), point(600, 89), switched, last]);
    assert_eq!(summary.cycles.len(), 1);
    assert_eq!(summary.cycles[0].evidence, CycleEvidence::ObservedCharge);
    assert_eq!(summary.cycles[0].awake_seconds, 600);
    assert_eq!(summary.cycles[0].consumed_percent, 2);
    assert_eq!(
        summary
            .rates
            .iter()
            .map(|rate| rate.consumed_percent)
            .sum::<u64>(),
        2
    );
}

#[test]
fn one_or_two_point_oscillation_does_not_manufacture_charge_or_drain() {
    for rebound in [1, 2] {
        let summary =
            summarize((0..100).map(|i| point(i * 300, if i % 2 == 0 { 80 } else { 80 - rebound })));
        assert_eq!(summary.cycles.len(), 1);
        assert_eq!(summary.cycles[0].evidence, CycleEvidence::Partial);
        assert_eq!(summary.cycles[0].consumed_percent, u64::from(rebound));
        assert_eq!(summary.rates[0].consumed_percent, u64::from(rebound));
        assert_eq!(summary.rates[0].drop_count, 1);
        assert!(summary.rates[0].projected_full_charge_hours.is_none());
    }
}

#[test]
fn unobserved_charge_across_sleep_is_inferred_without_counting_gap_drain() {
    for awake_level in [100, 69] {
        let mut sleeping = point(600, 70);
        sleeping.reading.connection = Connection::Sleeping;
        sleeping.reading.charging = None;
        let summary = summarize([
            point(0, 71),
            point(300, 70),
            sleeping,
            point(3600, awake_level),
        ]);
        assert_eq!(summary.rates[0].consumed_percent, 1);
        assert_eq!(summary.rates[0].awake_seconds, 300);
        if awake_level == 100 {
            assert_eq!(summary.cycles.len(), 2);
            assert_eq!(summary.cycles[1].evidence, CycleEvidence::InferredCharge);
            assert_eq!(summary.cycles[1].consumed_percent, 0);
            assert_eq!(summary.cycles[1].awake_seconds, 0);
        } else {
            assert_eq!(summary.cycles.len(), 1);
            assert_eq!(summary.cycles[0].consumed_percent, 1);
            assert_eq!(summary.cycles[0].awake_seconds, 300);
        }
    }
}
