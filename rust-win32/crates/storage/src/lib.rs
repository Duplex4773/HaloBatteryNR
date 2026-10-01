//! Bounded, batched history storage; configuration never shares upstream's folder.
use hb_core::{
    BatteryInsights, Estimator, HistoryStore, InsightsBuilder, PollingRate, ProviderError, Reading,
    Settings, Snapshot, UsageObservation,
};
use rusqlite::{Connection, params};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("HaloBatteryNext")
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), ProviderError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    let result = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
pub fn load_settings(path: &Path) -> Settings {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes)
            .map_err(|e| e.to_string())
            .and_then(Settings::from_value)
        {
            Ok(s) => s,
            Err(_) => {
                let _ = fs::rename(path, path.with_extension("json.bad"));
                Settings::default()
            }
        },
        Err(_) => Settings::default(),
    }
}
pub fn save_settings(path: &Path, settings: &Settings) -> Result<(), ProviderError> {
    atomic_write(
        path,
        &serde_json::to_vec_pretty(settings).map_err(|e| ProviderError::new(e.to_string()))?,
    )
}
pub fn write_status(path: &Path, snapshot: &Snapshot, running: bool) -> Result<(), ProviderError> {
    let devices: Vec<_> = snapshot.devices.iter().filter(|d| running && !d.hidden).map(|d| serde_json::json!({
        "key":d.reading.key,"name":d.name,"level":d.reading.level,"charging":d.reading.charging.unwrap_or(false),
        "online":d.reading.online(),"kind":d.icon,"approx":d.reading.approx,"low_alert_at":d.low_alert_at,
        "seconds_left":d.seconds_left,"text":d.text
    })).collect();
    atomic_write(path, &serde_json::to_vec(&serde_json::json!({"app":"Halo Battery Next","version":env!("CARGO_PKG_VERSION"),"running":running,
        "updated_unix":snapshot.timestamp,"updated":timestamp(snapshot.timestamp),"devices":devices})).map_err(|e| ProviderError::new(e.to_string()))?)
}
fn sql_error(e: rusqlite::Error) -> ProviderError {
    ProviderError::new(e.to_string())
}
/// UTC calendar rendering avoids locale-dependent output. All status fields match
/// upstream; the RFC3339 suffix makes the timestamp's timezone explicit.
fn timestamp(unix: i64) -> String {
    let days = unix.div_euclid(86400);
    let seconds = unix.rem_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}
pub struct Store {
    db: Connection,
    pending: Vec<UsageObservation>,
    last: BTreeMap<String, UsageObservation>,
    has_usage_metadata: bool,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self, ProviderError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let db = Connection::open(path).map_err(sql_error)?;
        db.busy_timeout(std::time::Duration::from_secs(2))
            .map_err(sql_error)?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA cache_size=-512;
            BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS readings(device TEXT NOT NULL, ts INTEGER NOT NULL, level INTEGER, payload TEXT NOT NULL, PRIMARY KEY(device,ts));
            CREATE INDEX IF NOT EXISTS readings_time ON readings(ts);
            CREATE TABLE IF NOT EXISTS state(key TEXT PRIMARY KEY, payload TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS usage_metadata(device TEXT NOT NULL, ts INTEGER NOT NULL, polling_rate INTEGER, session TEXT, PRIMARY KEY(device,ts));
            PRAGMA user_version=2; COMMIT;").map_err(sql_error)?;
        Ok(Self {
            db,
            pending: Vec::with_capacity(64),
            last: BTreeMap::new(),
            has_usage_metadata: true,
        })
    }
    pub fn read_only(path: &Path) -> Result<Self, ProviderError> {
        let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(sql_error)?;
        db.execute_batch("PRAGMA cache_size=-256;")
            .map_err(sql_error)?;
        let has_usage_metadata = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='usage_metadata')",
            [], |row| row.get(0),
        ).map_err(sql_error)?;
        Ok(Self {
            db,
            pending: Vec::new(),
            last: BTreeMap::new(),
            has_usage_metadata,
        })
    }
    pub fn prune(&mut self, now: i64) -> Result<(), ProviderError> {
        // Flush first so pending old observations cannot reappear after pruning.
        self.flush()?;
        let cutoff = now.saturating_sub(30 * 86400);
        let transaction = self.db.transaction().map_err(sql_error)?;
        transaction
            .execute("DELETE FROM readings WHERE ts < ?1", [cutoff])
            .map_err(sql_error)?;
        transaction.execute("DELETE FROM usage_metadata WHERE ts < ?1 OR NOT EXISTS(SELECT 1 FROM readings r WHERE r.device=usage_metadata.device AND r.ts=usage_metadata.ts)", [cutoff]).map_err(sql_error)?;
        transaction.commit().map_err(sql_error)?;
        self.last
            .retain(|_, observation| observation.reading.timestamp >= cutoff);
        Ok(())
    }
    /// Retained raw observations, streamed independently of chart sampling.
    /// As with history queries, flush pending observations before querying.
    pub fn query_insights(&self, key: &str, until: i64) -> Result<BatteryInsights, ProviderError> {
        let sql = if self.has_usage_metadata {
            "SELECT r.ts,r.payload,m.polling_rate,m.session FROM readings r LEFT JOIN usage_metadata m ON m.device=r.device AND m.ts=r.ts WHERE r.device=?1 AND r.ts BETWEEN ?2 AND ?3 ORDER BY r.ts"
        } else {
            "SELECT ts,payload,NULL,NULL FROM readings WHERE device=?1 AND ts BETWEEN ?2 AND ?3 ORDER BY ts"
        };
        let mut query = self.db.prepare(sql).map_err(sql_error)?;
        let mut rows = query
            .query(params![key, until.saturating_sub(30 * 86400), until])
            .map_err(sql_error)?;
        let mut builder = InsightsBuilder::default();
        while let Some(row) = rows.next().map_err(sql_error)? {
            let timestamp: i64 = row.get(0).map_err(sql_error)?;
            let payload: String = row.get(1).map_err(sql_error)?;
            let Ok(reading) = serde_json::from_str::<Reading>(&payload) else {
                builder.break_continuity();
                continue;
            };
            if reading.key != key || reading.timestamp != timestamp {
                builder.break_continuity();
                continue;
            }
            let polling_rate = row
                .get::<_, Option<u32>>(2)
                .ok()
                .flatten()
                .and_then(|hz| PollingRate::try_from(hz).ok());
            let session = row
                .get::<_, Option<String>>(3)
                .ok()
                .flatten()
                .and_then(|value| value.parse::<u64>().ok());
            builder.push(UsageObservation {
                reading,
                polling_rate,
                session,
            });
        }
        Ok(builder.finish())
    }
    /// Preserve confirmed configuration evidence without applying it to hardware.
    pub fn record_usage(&mut self, observation: &UsageObservation) -> Result<(), ProviderError> {
        let reading = &observation.reading;
        let changed = self.last.get(&reading.key).is_none_or(|previous| {
            let r = &previous.reading;
            previous.polling_rate != observation.polling_rate
                || previous.session != observation.session
                || r.level != reading.level
                || r.charging != reading.charging
                || r.charging_inferred != reading.charging_inferred
                || r.connection != reading.connection
                || r.precision != reading.precision
                || r.approx != reading.approx
                || r.name != reading.name
                || r.kind != reading.kind
                || r.source != reading.source
                || r.via != reading.via
                || r.serial != reading.serial
                || r.container != reading.container
                || reading.timestamp < r.timestamp
                || reading.timestamp.saturating_sub(r.timestamp) >= 60
        });
        if changed {
            self.last.insert(reading.key.clone(), observation.clone());
            self.pending.push(observation.clone());
        }
        if self.pending.len() >= 4096 {
            self.flush()?;
        }
        Ok(())
    }
    /// Chart history with at most one retained, valid last-known predecessor.
    /// Raw `HistoryStore::query` remains interval-only. No record is fabricated.
    pub fn query_with_baseline(
        &self,
        key: &str,
        since: i64,
        until: i64,
        max_points: usize,
    ) -> Result<Vec<Reading>, ProviderError> {
        let limit = max_points.clamp(2, 4096);
        let retained_since = until.saturating_sub(30 * 86400);
        let mut query = self.db.prepare(
            "SELECT ts,payload FROM readings WHERE device=?1 AND ts>=?2 AND ts<?3 AND level BETWEEN 0 AND 100 ORDER BY ts DESC",
        ).map_err(sql_error)?;
        let mut baseline = None;
        let mut rows = query
            .query(params![key, retained_since, since.min(until)])
            .map_err(sql_error)?;
        while let Some(row) = rows.next().map_err(sql_error)? {
            let timestamp: i64 = row.get(0).map_err(sql_error)?;
            let payload: String = row.get(1).map_err(sql_error)?;
            if let Ok(reading) = serde_json::from_str::<Reading>(&payload)
                && reading.key == key
                && reading.timestamp == timestamp
                && reading.level.is_some_and(|level| level <= 100)
            {
                baseline = Some(reading);
                break;
            }
        }
        let Some(baseline) = baseline else {
            return self.query(key, since, until, limit);
        };
        let mut interval = self.query(key, since, until, limit - 1)?;
        // The raw query reserves at least two endpoints. With a two-point chart,
        // retain the latest interval observation alongside the predecessor.
        if interval.len() > limit - 1 {
            interval = interval.into_iter().rev().take(limit - 1).collect();
            interval.reverse();
        }
        let mut out = Vec::with_capacity(interval.len() + 1);
        out.push(baseline);
        out.extend(interval);
        Ok(out)
    }
    /// Active-use history, summed from raw retained rows before display sampling.
    pub fn query_usage(
        &self,
        key: &str,
        until: i64,
        seconds: i64,
        max_points: usize,
    ) -> Result<hb_core::HistorySeries, ProviderError> {
        use hb_core::{HistoryAxis, HistorySample, HistorySeries};
        let limit = max_points.clamp(2, 4096);
        let total = self.stream_usage(key, until, |_, _| {})?;
        let since = total.saturating_sub(seconds.max(1)).max(0);
        let buckets = limit.saturating_sub(2) / 2;
        let mut extremes: Vec<Option<(HistorySample, HistorySample)>> = vec![None; buckets];
        let mut baseline = None;
        let mut first = None;
        let mut last = None;
        self.stream_usage(key, until, |reading, position| {
            if !reading.level.is_some_and(|level| level <= 100) {
                return;
            }
            let sample = HistorySample { reading, position };
            if position < since {
                baseline = Some(sample);
                return;
            }
            if first.is_none() {
                first = Some(baseline.take().unwrap_or_else(|| sample.clone()));
            }
            last = Some(sample.clone());
            if buckets > 0 {
                let span = total.saturating_sub(since).max(1);
                let bucket = ((position.saturating_sub(since) as i128 * buckets as i128)
                    / span as i128)
                    .min(buckets as i128 - 1) as usize;
                if let Some((low, high)) = &mut extremes[bucket] {
                    if sample.reading.level < low.reading.level {
                        *low = sample.clone();
                    }
                    if sample.reading.level > high.reading.level {
                        *high = sample;
                    }
                } else {
                    extremes[bucket] = Some((sample.clone(), sample));
                }
            }
        })?;
        let mut samples = Vec::with_capacity(limit);
        if let Some(sample) = first {
            samples.push(sample);
        }
        for (low, high) in extremes.into_iter().flatten() {
            samples.extend([low, high]);
        }
        if let Some(sample) = last {
            samples.push(sample);
        }
        samples.sort_by_key(|sample| sample.reading.timestamp);
        samples.dedup_by_key(|sample| sample.reading.timestamp);
        Ok(HistorySeries {
            samples,
            axis: HistoryAxis::Usage,
            since,
            until: total,
        })
    }
    fn stream_usage(
        &self,
        key: &str,
        until: i64,
        mut visit: impl FnMut(Reading, i64),
    ) -> Result<i64, ProviderError> {
        let mut query = self.db.prepare(
            "SELECT ts,payload FROM readings WHERE device=?1 AND ts BETWEEN ?2 AND ?3 ORDER BY ts",
        ).map_err(sql_error)?;
        let mut rows = query
            .query(params![key, until.saturating_sub(30 * 86400), until])
            .map_err(sql_error)?;
        let mut previous: Option<Reading> = None;
        let mut total = 0i64;
        while let Some(row) = rows.next().map_err(sql_error)? {
            let timestamp: i64 = row.get(0).map_err(sql_error)?;
            let payload: String = row.get(1).map_err(sql_error)?;
            let Ok(reading) = serde_json::from_str::<Reading>(&payload) else {
                previous = None;
                continue;
            };
            if reading.key != key || reading.timestamp != timestamp {
                previous = None;
                continue;
            }
            let awake = |r: &Reading| {
                r.online() && r.level.is_some_and(|level| level <= 100) && r.charging != Some(true)
            };
            if let Some(before) = &previous
                && awake(before)
                && awake(&reading)
            {
                total = total.saturating_add(
                    reading
                        .timestamp
                        .saturating_sub(before.timestamp)
                        .clamp(0, 600),
                );
            }
            visit(reading.clone(), total);
            previous = Some(reading);
        }
        Ok(total)
    }
    pub fn load_estimator(&self) -> Estimator {
        self.db
            .query_row("SELECT payload FROM state WHERE key='estimator'", [], |r| {
                r.get::<_, String>(0)
            })
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .map(|v| {
                Estimator::from_value(v, hb_core::Clock::unix(&hb_core::SystemClock::default()))
            })
            .unwrap_or_default()
    }
    pub fn save_estimator(&mut self, estimator: &Estimator) -> Result<(), ProviderError> {
        self.db
            .execute(
                "INSERT OR REPLACE INTO state(key,payload) VALUES('estimator',?1)",
                [serde_json::to_string(estimator)
                    .map_err(|e| ProviderError::new(e.to_string()))?],
            )
            .map_err(sql_error)?;
        Ok(())
    }
}
impl HistoryStore for Store {
    fn record(&mut self, reading: &Reading) -> Result<(), ProviderError> {
        self.record_usage(&UsageObservation {
            reading: reading.clone(),
            polling_rate: None,
            session: None,
        })
    }
    fn query(
        &self,
        key: &str,
        since: i64,
        until: i64,
        max_points: usize,
    ) -> Result<Vec<Reading>, ProviderError> {
        let limit = max_points.clamp(2, 4096);
        let count: i64 = self
            .db
            .query_row(
                "SELECT count(*) FROM readings WHERE device=?1 AND ts BETWEEN ?2 AND ?3",
                params![key, since, until],
                |r| r.get(0),
            )
            .map_err(sql_error)?;
        let mut out = Vec::with_capacity(limit.min(count as usize));
        if count > limit as i64 && limit == 2 {
            let mut query = self
                .db
                .prepare("SELECT payload FROM readings WHERE device=?1 AND ts=?2")
                .map_err(sql_error)?;
            let bounds = self
                .db
                .query_row(
                    "SELECT min(ts),max(ts) FROM readings WHERE device=?1 AND ts BETWEEN ?2 AND ?3",
                    params![key, since, until],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
                )
                .map_err(sql_error)?;
            for ts in [bounds.0, bounds.1] {
                let payload: String = query
                    .query_row(params![key, ts], |r| r.get(0))
                    .map_err(sql_error)?;
                if let Ok(r) = serde_json::from_str(&payload) {
                    out.push(r);
                }
            }
        } else if count <= limit as i64 {
            let mut query=self.db.prepare("SELECT payload FROM readings WHERE device=?1 AND ts BETWEEN ?2 AND ?3 ORDER BY ts").map_err(sql_error)?;
            for row in query
                .query_map(params![key, since, until], |r| r.get::<_, String>(0))
                .map_err(sql_error)?
            {
                if let Ok(r) = serde_json::from_str(&row.map_err(sql_error)?) {
                    out.push(r);
                }
            }
        } else {
            // SQL performs min/max sampling; an entire month is never loaded into UI memory.
            // A bucket can contain a low, high and unknown reading. Reserve room
            // for all three so null gaps never truncate the end of the interval.
            let buckets = (limit / 3).max(1) as i64;
            let width = (((until - since).max(0) + 1 + buckets - 1) / buckets).max(1);
            let mut query=self.db.prepare("WITH selected AS (SELECT ts,payload,level,(ts-?2)/?4 AS bucket FROM readings WHERE device=?1 AND ts BETWEEN ?2 AND ?3), extremes AS (SELECT bucket,min(level) AS lo,max(level) AS hi FROM selected GROUP BY bucket) SELECT s.payload FROM selected s JOIN extremes e ON s.bucket=e.bucket WHERE s.level=e.lo OR s.level=e.hi OR s.level IS NULL GROUP BY s.bucket,s.level ORDER BY s.ts LIMIT ?5").map_err(sql_error)?;
            for row in query
                .query_map(params![key, since, until, width, limit as i64], |r| {
                    r.get::<_, String>(0)
                })
                .map_err(sql_error)?
            {
                if let Ok(r) = serde_json::from_str(&row.map_err(sql_error)?) {
                    out.push(r);
                }
            }
        }
        Ok(out)
    }
    fn flush(&mut self) -> Result<(), ProviderError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let transaction = self.db.transaction().map_err(sql_error)?;
        for observation in &self.pending {
            let r = &observation.reading;
            transaction
                .execute(
                    "INSERT OR REPLACE INTO readings(device,ts,level,payload) VALUES(?1,?2,?3,?4)",
                    params![
                        r.key,
                        r.timestamp,
                        r.level,
                        serde_json::to_string(r).map_err(|e| ProviderError::new(e.to_string()))?
                    ],
                )
                .map_err(sql_error)?;
            transaction.execute(
                "INSERT OR REPLACE INTO usage_metadata(device,ts,polling_rate,session) VALUES(?1,?2,?3,?4)",
                params![r.key, r.timestamp, observation.polling_rate.map(PollingRate::hz), observation.session.map(|session| session.to_string())],
            ).map_err(sql_error)?;
        }
        transaction.commit().map_err(sql_error)?;
        self.pending.clear();
        Ok(())
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corruption_is_preserved_and_atomic_writes_replace() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        fs::write(&p, b"broken").unwrap();
        assert_eq!(load_settings(&p).interval, 60);
        assert!(p.with_extension("json.bad").exists());
        let s = Settings {
            low: 15,
            ..Default::default()
        };
        save_settings(&p, &s).unwrap();
        save_settings(&p, &Settings::default()).unwrap();
        assert_eq!(load_settings(&p).low, 20);
    }
    #[test]
    fn failed_atomic_save_preserves_previous_configuration() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        save_settings(&p, &Settings::default()).unwrap();
        let previous = fs::read(&p).unwrap();
        // A directory occupying the temporary filename causes a real filesystem
        // failure before replacement, without permission assumptions on CI.
        fs::create_dir(p.with_extension("tmp")).unwrap();
        let changed = Settings {
            low: 10,
            ..Default::default()
        };
        assert!(save_settings(&p, &changed).is_err());
        assert_eq!(fs::read(&p).unwrap(), previous);
        assert_eq!(load_settings(&p).low, 20);
    }
    #[test]
    fn configuration_roundtrip_preserves_unicode_and_device_preferences() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        let mut settings = Settings {
            interval: 120,
            animation: false,
            ..Default::default()
        };
        settings.devices.insert(
            "hardware-id".into(),
            hb_core::DevicePreferences {
                name: Some("Souris préférée 🖱".into()),
                hidden: true,
                low: Some(12),
                icon: Some("mouse".into()),
                ..Default::default()
            },
        );
        save_settings(&p, &settings).unwrap();
        let actual = load_settings(&p);
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(settings).unwrap()
        );
    }
    #[test]
    fn samples_are_bounded_persistent_and_pruned() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("history.db");
        let mut store = Store::open(&p).unwrap();
        for ts in 0..1000 {
            let mut r = Reading::new("one", "Mouse", "razer", ts);
            r.level = Some((ts % 100) as u8);
            store.record(&r).unwrap();
        }
        store.flush().unwrap();
        assert!(store.query("one", 0, 999, 20).unwrap().len() <= 20);
        store.prune(31 * 86400).unwrap();
        assert!(store.query("one", 0, 999, 20).unwrap().is_empty());
    }
    #[test]
    fn unchanged_readings_are_not_recorded_more_than_once_a_minute() {
        let d = tempfile::tempdir().unwrap();
        let mut s = Store::open(&d.path().join("history.db")).unwrap();
        for ts in 0..120 {
            let mut r = Reading::new("one", "Mouse", "razer", ts);
            r.level = Some(80);
            s.record(&r).unwrap();
        }
        s.flush().unwrap();
        assert_eq!(s.query("one", 0, 120, 100).unwrap().len(), 2);
    }
    #[test]
    fn precision_and_charging_evidence_changes_are_recorded_without_waiting_a_minute() {
        let d = tempfile::tempdir().unwrap();
        let mut s = Store::open(&d.path().join("history.db")).unwrap();
        let mut r = Reading::new("one", "Mouse", "razer", 100);
        r.level = Some(50);
        r.charging = Some(true);
        s.record(&r).unwrap();
        r.timestamp += 1;
        r.precision = hb_core::Precision::Coarse;
        r.approx = Some("about half".into());
        s.record(&r).unwrap();
        r.timestamp += 1;
        r.charging_inferred = true;
        s.record(&r).unwrap();
        r.timestamp += 1;
        r.name = "Updated device model".into();
        s.record(&r).unwrap();
        s.flush().unwrap();
        let rows = s.query("one", 100, 103, 20).unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].precision, hb_core::Precision::Exact);
        assert_eq!(rows[1].approx.as_deref(), Some("about half"));
        assert!(!rows[1].charging_inferred);
        assert!(rows[2].charging_inferred);
        assert_eq!(rows[3].name, "Updated device model");
    }
    #[test]
    fn null_gaps_do_not_truncate_the_requested_interval() {
        let d = tempfile::tempdir().unwrap();
        let mut s = Store::open(&d.path().join("history.db")).unwrap();
        for ts in 0..3000 {
            let mut r = Reading::new("one", "Mouse", "razer", ts);
            r.level = if ts % 3 == 0 {
                None
            } else {
                Some((ts % 100) as u8)
            };
            s.record(&r).unwrap();
        }
        s.flush().unwrap();
        let points = s.query("one", 0, 2999, 60).unwrap();
        assert!(points.len() <= 60);
        assert!(points.last().unwrap().timestamp >= 2850);
        let endpoints = s.query("one", 0, 2999, 2).unwrap();
        assert_eq!(endpoints[0].timestamp, 0);
        assert_eq!(endpoints[1].timestamp, 2999);
        assert!(s.query("other", 0, 2999, 100).unwrap().is_empty());
    }
    #[test]
    fn shutdown_drop_flushes_and_backward_clock_restarts_pacing() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("history.db");
        {
            let mut s = Store::open(&path).unwrap();
            let mut r = Reading::new("one", "Mouse", "razer", 1000);
            r.level = Some(60);
            s.record(&r).unwrap();
            r.timestamp = 100;
            s.record(&r).unwrap();
        }
        let s = Store::read_only(&path).unwrap();
        assert_eq!(s.query("one", 0, 1000, 100).unwrap().len(), 2);
    }
    #[test]
    fn status_schema_filters_hidden_and_preserves_nullable_percentage() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("status.json");
        let mut engine = hb_core::Engine::new(Settings::default(), Estimator::default());
        let a = Reading::new("unknown", "Controller", "xinput", 0);
        let b = Reading::new("hidden", "Mouse", "razer", 0);
        engine.settings.devices.insert(
            "hidden".into(),
            hb_core::DevicePreferences {
                hidden: true,
                ..Default::default()
            },
        );
        engine.apply("xinput", Ok(vec![a]), 0., false);
        engine.apply("razer", Ok(vec![b]), 0., false);
        write_status(&path, &engine.snapshot(0), true).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["updated"], "1970-01-01T00:00:00Z");
        assert_eq!(value["devices"].as_array().unwrap().len(), 1);
        assert!(value["devices"][0]["level"].is_null());
        assert_eq!(value["running"], true);
    }
    #[test]
    fn status_values_include_renames_thresholds_types_and_estimates() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("status.json");
        let mut settings = Settings::default();
        settings.devices.insert(
            "mouse".into(),
            hb_core::DevicePreferences {
                name: Some("My mouse".into()),
                icon: Some("headphones".into()),
                low: Some(13),
                ..Default::default()
            },
        );
        let mut engine = hb_core::Engine::new(settings, Estimator::default());
        let mut reading = Reading::new("mouse", "Factory name", "razer", 1_780_000_000);
        reading.level = Some(42);
        reading.charging = Some(true);
        reading.approx = Some("Medium".into());
        engine.apply("razer", Ok(vec![reading]), 0., false);
        let mut snapshot = engine.snapshot(1_780_000_000);
        snapshot.devices[0].seconds_left = Some(3600);
        snapshot.devices[0].text = "My mouse: Medium · 1h left".into();
        write_status(&path, &snapshot, true).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            value["devices"][0],
            serde_json::json!({
                "key":"mouse", "name":"My mouse", "level":42, "charging":true,
                "online":true, "kind":"headphones", "approx":"Medium",
                "low_alert_at":13, "seconds_left":3600, "text":"My mouse: Medium · 1h left"
            })
        );
        assert_eq!(value["updated_unix"], 1_780_000_000);
        write_status(&path, &Snapshot::default(), false).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["devices"], serde_json::json!([]));
        assert_eq!(value["running"], false);
    }
    fn baseline_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("baseline.db")).unwrap();
        (dir, store)
    }
    fn seed(store: &mut Store, key: &str, timestamp: i64, level: Option<u8>) {
        let mut reading = Reading::new(key, "Mouse", "test", timestamp);
        reading.level = level;
        reading.connection = hb_core::Connection::Sleeping;
        store.record(&reading).unwrap();
        store.flush().unwrap();
    }
    #[test]
    fn sleeping_interval_receives_original_last_known_baseline_only() {
        let (_dir, mut store) = baseline_store();
        seed(&mut store, "a", 90, Some(73));
        let result = store.query_with_baseline("a", 100, 200, 100).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].timestamp, 90);
        assert_eq!(result[0].level, Some(73));
        assert_eq!(result[0].connection, hb_core::Connection::Sleeping);
        assert!(store.query("a", 100, 200, 100).unwrap().is_empty());
    }
    #[test]
    fn baseline_skips_unknown_invalid_and_corrupt_predecessors_and_other_devices() {
        let (_dir, mut store) = baseline_store();
        seed(&mut store, "a", 70, Some(60));
        seed(&mut store, "a", 80, Some(101));
        seed(&mut store, "a", 90, None);
        seed(&mut store, "b", 99, Some(95));
        store
            .db
            .execute("INSERT INTO readings VALUES('a',95,50,'broken')", [])
            .unwrap();
        let result = store.query_with_baseline("a", 100, 200, 2).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            (result[0].key.as_str(), result[0].timestamp, result[0].level),
            ("a", 70, Some(60))
        );
    }
    #[test]
    fn baseline_retention_is_relative_to_interval_end_and_inclusive() {
        let (_dir, mut store) = baseline_store();
        let until = 30 * 86400 + 100;
        seed(&mut store, "a", 99, Some(50));
        assert!(
            store
                .query_with_baseline("a", until - 20, until, 20)
                .unwrap()
                .is_empty()
        );
        seed(&mut store, "a", 100, Some(40));
        assert_eq!(
            store
                .query_with_baseline("a", until - 20, until, 20)
                .unwrap()[0]
                .timestamp,
            100
        );
    }
    #[test]
    fn baseline_budget_preserves_interval_endpoints_and_never_exceeds_cap() {
        let (_dir, mut store) = baseline_store();
        seed(&mut store, "a", 90, Some(80));
        for ts in 100..120 {
            seed(&mut store, "a", ts, Some((ts - 50) as u8));
        }
        let two = store.query_with_baseline("a", 100, 119, 2).unwrap();
        assert_eq!(
            two.iter().map(|r| r.timestamp).collect::<Vec<_>>(),
            vec![90, 119]
        );
        let three = store.query_with_baseline("a", 100, 119, 3).unwrap();
        assert_eq!(
            three.iter().map(|r| r.timestamp).collect::<Vec<_>>(),
            vec![90, 100, 119]
        );
        let all = store.query_with_baseline("a", 100, 119, 30).unwrap();
        assert_eq!(all.len(), 21);
        assert_eq!(all[1].timestamp, 100);
        assert_eq!(all.last().unwrap().timestamp, 119);
        for cap in [0, 1, 2, 3, 4, 8, 4096, 5000] {
            let result = store.query_with_baseline("a", 100, 119, cap).unwrap();
            assert!(result.len() <= cap.clamp(2, 4096));
            assert_eq!(result[0].timestamp, 90);
        }
    }
    fn usage_seed(
        store: &mut Store,
        ts: i64,
        level: Option<u8>,
        connection: hb_core::Connection,
        charging: Option<bool>,
    ) {
        let mut r = Reading::new("usage", "Mouse", "test", ts);
        r.level = level;
        r.connection = connection;
        r.charging = charging;
        store.record(&r).unwrap();
        store.flush().unwrap();
    }
    #[test]
    fn usage_pauses_sleep_charging_unknown_and_resumes_after_wake() {
        use hb_core::Connection::{Online, Sleeping};
        let (_dir, mut store) = baseline_store();
        for (ts, level, connection, charging) in [
            (0, Some(80), Online, None),
            (60, Some(79), Online, None),
            (120, Some(79), Sleeping, None),
            (10000, Some(79), Sleeping, None),
            (10060, Some(79), Online, None),
            (10120, Some(78), Online, None),
            (10180, Some(78), Online, Some(true)),
            (10240, Some(80), Online, Some(true)),
            (10300, Some(80), Online, None),
            (10360, None, Online, None),
            (10420, Some(79), Online, None),
            (10480, Some(78), Online, None),
        ] {
            usage_seed(&mut store, ts, level, connection, charging);
        }
        let series = store.query_usage("usage", 10480, 3600, 100).unwrap();
        assert_eq!((series.since, series.until), (0, 180));
        assert_eq!(series.axis, hb_core::HistoryAxis::Usage);
        assert_eq!(series.samples.last().unwrap().reading.timestamp, 10480);
        assert_eq!(series.samples.last().unwrap().position, 180);
    }
    #[test]
    fn usage_clamps_app_gaps_and_sampling_does_not_change_total_or_device_scope() {
        let (_dir, mut store) = baseline_store();
        usage_seed(&mut store, 0, Some(100), hb_core::Connection::Online, None);
        usage_seed(
            &mut store,
            10000,
            Some(90),
            hb_core::Connection::Online,
            None,
        );
        for i in 1..101 {
            usage_seed(
                &mut store,
                10000 + i * 60,
                Some((90 - i % 80) as u8),
                hb_core::Connection::Online,
                None,
            );
        }
        seed(&mut store, "other", 16001, Some(50));
        for cap in [2, 3, 4, 10, 4096] {
            let series = store.query_usage("usage", 16001, 1200, cap).unwrap();
            assert_eq!((series.since, series.until), (5400, 6600));
            assert!(series.samples.len() <= cap);
            assert_eq!(series.samples.last().unwrap().reading.timestamp, 16000);
            assert_eq!(series.samples.last().unwrap().position, 6600);
            assert!(series.samples.iter().all(|s| s.reading.key == "usage"));
            assert!(series.samples.first().unwrap().position <= 5400);
        }
    }
    #[test]
    fn usage_empty_sleep_only_and_retention_are_safe() {
        let (_dir, mut store) = baseline_store();
        assert!(
            store
                .query_usage("usage", 100, 0, 0)
                .unwrap()
                .samples
                .is_empty()
        );
        usage_seed(
            &mut store,
            10,
            Some(50),
            hb_core::Connection::Sleeping,
            None,
        );
        let series = store.query_usage("usage", 100, 0, 2).unwrap();
        assert_eq!((series.since, series.until), (0, 0));
        assert_eq!(series.samples[0].reading.timestamp, 10);
        assert!(
            store
                .query_usage("usage", 30 * 86400 + 11, 100, 10)
                .unwrap()
                .samples
                .is_empty()
        );
    }
    #[test]
    fn usage_stale_invalid_and_corrupt_rows_break_interval_continuity() {
        let (_dir, mut store) = baseline_store();
        for (ts, level, state) in [
            (0, Some(50), hb_core::Connection::Online),
            (60, Some(101), hb_core::Connection::Online),
            (120, Some(50), hb_core::Connection::Online),
            (180, Some(50), hb_core::Connection::Stale),
            (240, Some(50), hb_core::Connection::Online),
            (360, Some(50), hb_core::Connection::Online),
            (420, Some(49), hb_core::Connection::Online),
        ] {
            usage_seed(&mut store, ts, level, state, None);
        }
        store
            .db
            .execute("INSERT INTO readings VALUES('usage',300,50,'broken')", [])
            .unwrap();
        let series = store.query_usage("usage", 420, 3600, 20).unwrap();
        assert_eq!(series.until, 60);
        assert!(
            series
                .samples
                .iter()
                .all(|s| s.reading.level.is_some_and(|level| level <= 100))
        );
    }
    #[test]
    #[ignore = "explicit synthetic 30-day query timing; no hardware"]
    fn usage_thirty_day_stream_timing() {
        let (_dir, mut store) = baseline_store();
        let transaction = store.db.transaction().unwrap();
        {
            let mut insert = transaction
                .prepare("INSERT INTO readings VALUES(?1,?2,?3,?4)")
                .unwrap();
            for i in 0..43200i64 {
                let mut r = Reading::new("usage", "Mouse", "test", i * 60);
                r.level = Some((100 - i % 100) as u8);
                insert
                    .execute(params![
                        r.key,
                        r.timestamp,
                        r.level,
                        serde_json::to_string(&r).unwrap()
                    ])
                    .unwrap();
            }
        }
        transaction.commit().unwrap();
        let start = std::time::Instant::now();
        let series = store
            .query_usage("usage", 43199 * 60, 12 * 3600, 1000)
            .unwrap();
        println!("43200 rows, two passes: {:?}", start.elapsed());
        assert_eq!(series.until, 43199 * 60);
        assert!(series.samples.len() <= 1000);
    }
}
