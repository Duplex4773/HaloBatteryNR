//! Bounded, batched history storage; configuration never shares upstream's folder.
use hb_core::{Estimator, HistoryStore, ProviderError, Reading, Settings, Snapshot};
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
    pending: Vec<Reading>,
    last: BTreeMap<String, Reading>,
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
            CREATE TABLE IF NOT EXISTS readings(device TEXT NOT NULL, ts INTEGER NOT NULL, level INTEGER, payload TEXT NOT NULL, PRIMARY KEY(device,ts));
            CREATE INDEX IF NOT EXISTS readings_time ON readings(ts);
            CREATE TABLE IF NOT EXISTS state(key TEXT PRIMARY KEY, payload TEXT NOT NULL);
            PRAGMA user_version=1;").map_err(sql_error)?;
        Ok(Self {
            db,
            pending: Vec::with_capacity(64),
            last: BTreeMap::new(),
        })
    }
    pub fn read_only(path: &Path) -> Result<Self, ProviderError> {
        let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(sql_error)?;
        db.execute_batch("PRAGMA cache_size=-256;")
            .map_err(sql_error)?;
        Ok(Self {
            db,
            pending: Vec::new(),
            last: BTreeMap::new(),
        })
    }
    pub fn prune(&mut self, now: i64) -> Result<(), ProviderError> {
        self.db
            .execute("DELETE FROM readings WHERE ts < ?1", [now - 30 * 86400])
            .map_err(sql_error)?;
        Ok(())
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
        let changed = self.last.get(&reading.key).is_none_or(|r| {
            r.level != reading.level
                || r.charging != reading.charging
                || r.connection != reading.connection
                || reading.timestamp < r.timestamp
                || reading.timestamp - r.timestamp >= 60
        });
        if changed {
            self.last.insert(reading.key.clone(), reading.clone());
            self.pending.push(reading.clone());
        }
        if self.pending.len() >= 4096 {
            self.flush()?;
        }
        Ok(())
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
        for r in &self.pending {
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
}
