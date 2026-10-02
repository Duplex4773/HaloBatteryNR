use hb_core::{
    Connection as DeviceConnection, HistoryStore, InsightsBuilder, PollingRate, Reading,
    UsageObservation,
};
use hb_storage::Store;
use rusqlite::{Connection, params};

fn observation(ts: i64, level: u8, hz: Option<u32>, session: Option<u64>) -> UsageObservation {
    let mut reading = Reading::new("synthetic-a", "Test mouse", "test", ts);
    reading.connection = DeviceConnection::Online;
    reading.level = Some(level);
    reading.charging = Some(false);
    UsageObservation {
        reading,
        polling_rate: hz.map(|rate| PollingRate::try_from(rate).unwrap()),
        session,
    }
}

#[test]
fn pending_metadata_survives_drop_restart_and_rate_session_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.db");
    let samples = [
        observation(0, 90, Some(1000), Some(u64::MAX)),
        observation(1, 90, Some(500), Some(u64::MAX)),
        observation(2, 90, Some(500), Some(7)),
        observation(62, 89, Some(500), Some(7)),
        observation(122, 88, Some(500), Some(7)),
    ];
    {
        let mut store = Store::open(&path).unwrap();
        for sample in &samples {
            store.record_usage(sample).unwrap();
        }
        // Repeated polls in the same state do not create additional rows.
        store
            .record_usage(&observation(123, 88, Some(500), Some(7)))
            .unwrap();
    }
    let db = Connection::open(&path).unwrap();
    let stored: (i64, String) = db
        .query_row(
            "SELECT polling_rate,session FROM usage_metadata WHERE ts=0",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored, (1000, u64::MAX.to_string()));
    let store = Store::read_only(&path).unwrap();
    assert_eq!(
        store.query("synthetic-a", 0, 200, 100).unwrap().len(),
        samples.len()
    );
    let mut expected = InsightsBuilder::default();
    for sample in samples {
        expected.push(sample);
    }
    let expected = expected.finish();
    let actual = store.query_insights("synthetic-a", 200).unwrap();
    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    assert!(
        store
            .query_insights("synthetic-other", 200)
            .unwrap()
            .rates
            .is_empty()
    );
}

#[test]
fn same_second_metadata_replacement_and_legacy_record_clear_evidence_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.db");
    let mut store = Store::open(&path).unwrap();
    store
        .record_usage(&observation(100, 80, Some(1000), Some(1)))
        .unwrap();
    store
        .record_usage(&observation(100, 80, Some(500), Some(2)))
        .unwrap();
    store.flush().unwrap();
    let db = Connection::open(&path).unwrap();
    let metadata: (i64, String) = db
        .query_row(
            "SELECT polling_rate,session FROM usage_metadata",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(metadata, (500, "2".into()));
    store
        .record(&observation(100, 80, None, None).reading)
        .unwrap();
    store.flush().unwrap();
    let metadata: (Option<i64>, Option<String>) = db
        .query_row(
            "SELECT polling_rate,session FROM usage_metadata",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(metadata, (None, None));
    assert_eq!(store.query("synthetic-a", 0, 200, 100).unwrap().len(), 1);
}

fn legacy_database(path: &std::path::Path) {
    let db = Connection::open(path).unwrap();
    db.execute_batch("CREATE TABLE readings(device TEXT NOT NULL, ts INTEGER NOT NULL, level INTEGER, payload TEXT NOT NULL, PRIMARY KEY(device,ts)); CREATE TABLE state(key TEXT PRIMARY KEY,payload TEXT NOT NULL); PRAGMA user_version=1;").unwrap();
    for sample in [
        observation(0, 90, None, None),
        observation(60, 89, None, None),
    ] {
        db.execute(
            "INSERT INTO readings VALUES(?1,?2,?3,?4)",
            params![
                sample.reading.key,
                sample.reading.timestamp,
                sample.reading.level,
                serde_json::to_string(&sample.reading).unwrap()
            ],
        )
        .unwrap();
    }
}

#[test]
fn old_database_is_readable_without_upgrade_and_upgrades_preserving_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.db");
    legacy_database(&path);
    {
        let store = Store::read_only(&path).unwrap();
        assert_eq!(store.query("synthetic-a", 0, 60, 100).unwrap().len(), 2);
        assert!(
            store
                .query_insights("synthetic-a", 60)
                .unwrap()
                .rates
                .is_empty()
        );
    }
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.query("synthetic-a", 0, 60, 100).unwrap().len(), 2);
    store
        .record_usage(&observation(120, 88, Some(1000), Some(1)))
        .unwrap();
    store.flush().unwrap();
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM usage_metadata", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn prune_flushes_pending_and_removes_metadata_at_retention_boundary() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.db");
    let mut store = Store::open(&path).unwrap();
    for (ts, level) in [(99, 80), (100, 81), (160, 82)] {
        store
            .record_usage(&observation(ts, level, Some(1000), Some(1)))
            .unwrap();
    }
    store.prune(30 * 86400 + 100).unwrap();
    store.flush().unwrap();
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT min(ts) FROM readings", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        100
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM usage_metadata", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    let actual = store
        .query_insights("synthetic-a", 30 * 86400 + 101)
        .unwrap();
    let mut expected = InsightsBuilder::default();
    expected.push(observation(160, 82, Some(1000), Some(1)));
    assert_eq!(format!("{actual:?}"), format!("{:?}", expected.finish()));
}

#[test]
fn retained_raw_rows_reach_aggregation_without_chart_sampling() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.db");
    let mut store = Store::open(&path).unwrap();
    let mut expected = InsightsBuilder::default();
    for index in 0..43_200i64 {
        // Full month, monotonically repeated discharge segments, bounded builder output.
        let sample = observation(index * 60, (100 - index % 100) as u8, Some(1000), Some(1));
        store.record_usage(&sample).unwrap();
        expected.push(sample);
    }
    store.flush().unwrap();
    let actual = store.query_insights("synthetic-a", 43_199 * 60).unwrap();
    assert_eq!(format!("{actual:?}"), format!("{:?}", expected.finish()));
    assert!(
        store
            .query("synthetic-a", 0, 43_199 * 60, 20)
            .unwrap()
            .len()
            <= 20
    );
}

#[test]
fn metadata_failure_rolls_back_reading_and_preserves_pending_for_retry() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.db");
    let mut store = Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER reject_metadata BEFORE INSERT ON usage_metadata BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    store
        .record_usage(&observation(100, 80, Some(1000), Some(1)))
        .unwrap();
    assert!(store.flush().is_err());
    assert!(store.query("synthetic-a", 0, 200, 100).unwrap().is_empty());
    db.execute_batch("DROP TRIGGER reject_metadata;").unwrap();
    store.flush().unwrap();
    assert_eq!(store.query("synthetic-a", 0, 200, 100).unwrap().len(), 1);
    assert_eq!(
        db.query_row("SELECT count(*) FROM usage_metadata", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn corrupt_raw_rows_break_insight_continuity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.db");
    let mut store = Store::open(&path).unwrap();
    let first = observation(0, 90, Some(1000), Some(1));
    let last = observation(120, 88, Some(1000), Some(1));
    store.record_usage(&first).unwrap();
    store.record_usage(&last).unwrap();
    store.flush().unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "INSERT INTO readings VALUES('synthetic-a',60,89,'corrupt')",
        [],
    )
    .unwrap();
    let mut expected = InsightsBuilder::default();
    expected.push(first);
    expected.break_continuity();
    expected.push(last);
    let actual = store.query_insights("synthetic-a", 120).unwrap();
    assert_eq!(format!("{actual:?}"), format!("{:?}", expected.finish()));
}

#[test]
fn old_rate_evidence_is_historical_not_current_remaining_use() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.db");
    let mut store = Store::open(&path).unwrap();
    for i in 0..=4 {
        store
            .record_usage(&observation(i * 600, 80 - i as u8, Some(1000), Some(1)))
            .unwrap();
    }
    store.flush().unwrap();
    let fresh = store.query_insights("synthetic-a", 2400).unwrap();
    let old = store.query_insights("synthetic-a", 3100).unwrap();
    assert!(fresh.rates[0].remaining_hours.is_some());
    assert!(old.rates[0].remaining_hours.is_none());
    assert_eq!(
        fresh.rates[0].projected_full_charge_hours,
        old.rates[0].projected_full_charge_hours
    );
    assert_eq!(old.coverage.observation_count, 5);
    assert_eq!(old.coverage.last_reading_timestamp, Some(2400));
}
