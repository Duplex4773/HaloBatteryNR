//! A single UI thread owns all HWND, HICON and Direct2D resources.
use crate::{
    chart::Chart,
    dashboard_theme::{DashboardTheme, Palette, color_brush},
    icons::{self, Icon},
    runtime::{Command, Event, Runtime},
};
use hb_core::*;
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    collections::BTreeMap,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Devices::HumanInterfaceDevice::HidD_GetHidGuid,
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{
                DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_CHECKED, ODS_DISABLED, ODS_GRAYED,
                ODS_NOACCEL, ODS_SELECTED, ODT_MENU,
            },
            HiDpi::*,
            Input::KeyboardAndMouse::EnableWindow,
            Shell::*,
            WindowsAndMessaging::*,
        },
    },
    core::{GUID, PCWSTR, w},
};
const TRAY: u32 = WM_APP + 1;
fn providers() -> Vec<&'static str> {
    hb_providers::provider::FAMILIES
        .iter()
        .map(|p| p.0)
        .chain(["bluetooth", "xinput"])
        .collect()
}
const CHECKS: &[(&str, &str)] = &[
    ("notify", "Battery notifications"),
    ("full_alert", "Full charge alert"),
    ("bluetooth", "Bluetooth devices"),
    ("animation", "Charging animation"),
    ("badges", "Charging badge"),
    ("fluent_menu", "Device flyout"),
    ("time_left", "Estimated time remaining"),
    ("percent_in_icon", "Percentage in icon"),
    ("quiet_fullscreen", "Quiet while gaming / presenting"),
    ("status_file", "Write status.json"),
    (
        "playstation_full_mode",
        "PlayStation full Bluetooth mode (opt in)",
    ),
    (
        "update_check",
        "Release checks (not available in this build)",
    ),
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrayUpdate {
    Changed,
    Redraw,
    ExplorerRecovery,
}
#[derive(Default)]
struct TrayThemeCache {
    value: Option<(Instant, String, bool)>,
}
#[cfg(test)]
mod tray_theme_cache_tests {
    use super::*;
    #[test]
    fn caches_short_snapshot_bursts_but_rechecks_settings_recovery_and_expiry() {
        let mut cache = TrayThemeCache::default();
        let now = Instant::now();
        let calls = Cell::new(0);
        let sample = || {
            calls.set(calls.get() + 1);
            true
        };
        assert!(cache.read(now, "auto", false, sample));
        assert!(
            cache.read(now + Duration::from_millis(500), "auto", false, || panic!(
                "cached"
            ))
        );
        assert!(cache.read(now + Duration::from_secs(2), "auto", false, sample));
        assert!(cache.read(now + Duration::from_secs(2), "auto", true, sample));
        assert!(cache.read(now + Duration::from_secs(2), "black", false, sample));
        assert_eq!(calls.get(), 4);
    }
}
impl TrayThemeCache {
    fn read(
        &mut self,
        now: Instant,
        icon_theme: &str,
        force: bool,
        query: impl FnOnce() -> bool,
    ) -> bool {
        if !force
            && let Some((sampled, key, value)) = &self.value
            && key == icon_theme
            && now.saturating_duration_since(*sampled) < Duration::from_secs(2)
        {
            return *value;
        }
        let value = query();
        self.value = Some((now, icon_theme.to_owned(), value));
        value
    }
}
fn tray_message_update(message: u32, taskbar_created: u32) -> Option<TrayUpdate> {
    if taskbar_created != 0 && message == taskbar_created {
        Some(TrayUpdate::ExplorerRecovery)
    } else if message == WM_SETTINGCHANGE {
        Some(TrayUpdate::Redraw)
    } else {
        None
    }
}
#[derive(Default)]
struct TrayRegistration {
    registered: bool,
}
impl TrayRegistration {
    fn update_with(
        &mut self,
        data: &NOTIFYICONDATAW,
        update: TrayUpdate,
        changed: bool,
        notify: &mut impl FnMut(NOTIFY_ICON_MESSAGE, &NOTIFYICONDATAW) -> bool,
    ) {
        if !self.registered {
            self.registered = notify(NIM_ADD, data);
        } else if (changed || update != TrayUpdate::Changed)
            && !notify(NIM_MODIFY, data)
            && update == TrayUpdate::ExplorerRecovery
        {
            // Explorer may still own the GUID. Add only after it reports the
            // icon missing; deleting an existing icon loses its pinned placement.
            self.registered = notify(NIM_ADD, data);
        }
    }
    fn remove_with(
        &mut self,
        data: &NOTIFYICONDATAW,
        notify: &mut impl FnMut(NOTIFY_ICON_MESSAGE, &NOTIFYICONDATAW) -> bool,
    ) {
        if self.registered {
            let _ = notify(NIM_DELETE, data);
            self.registered = false;
        }
    }
}
fn shell_notify(command: NOTIFY_ICON_MESSAGE, data: &NOTIFYICONDATAW) -> bool {
    unsafe { Shell_NotifyIconW(command, data) }.as_bool()
}
struct Tray {
    registration: TrayRegistration,
    data: NOTIFYICONDATAW,
    frames: Vec<Icon>,
    signature: IconSignature,
    frame: usize,
}
impl Drop for Tray {
    fn drop(&mut self) {
        self.registration.remove_with(&self.data, &mut shell_notify);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PollingIntent {
    Read,
    Apply { rate: PollingRate, restore: bool },
}
#[derive(Clone, Debug)]
struct PendingPolling {
    request: u64,
    key: String,
    device: ConfigurationDevice,
    intent: PollingIntent,
}
#[derive(Default)]
struct PollingUi {
    sequence: u64,
    generation: u64,
    pending: Option<PendingPolling>,
    observations: BTreeMap<String, PollingObservation>,
    previous: BTreeMap<String, PollingRate>,
    status: BTreeMap<String, String>,
}
impl PollingUi {
    fn revoke_observation(&mut self, key: &str) {
        self.observations.remove(key);
        self.status
            .insert(key.into(), "Hardware rate needs verification.".into());
    }
    fn abandon(&mut self) {
        self.pending = None;
    }
    fn invalidate(&mut self, generation: u64) {
        if generation < self.generation {
            return;
        }
        self.generation = generation;
        self.observations.clear();
        self.previous.clear();
        self.status.clear();
        self.pending = None;
    }
    #[cfg(test)]
    fn retain_devices(&mut self, devices: &[ConfigurationDevice]) {
        self.retain_inventory(&[], devices);
    }
    fn retain_inventory(
        &mut self,
        batteries: &[DeviceView],
        configuration: &[ConfigurationDevice],
    ) {
        let mut keys = std::collections::BTreeSet::new();
        for key in batteries
            .iter()
            .map(|d| d.reading.key.as_str())
            .chain(configuration.iter().map(|d| d.key.as_str()))
        {
            if keys.len() < 512 {
                keys.insert(key);
            }
        }
        self.observations.retain(|key, observation| {
            keys.contains(key.as_str())
                && inventory_contains_device(batteries, configuration, &observation.target.device)
        });
        self.previous.retain(|key, _| keys.contains(key.as_str()));
        self.status.retain(|key, _| keys.contains(key.as_str()));
        if self.pending.as_ref().is_some_and(|pending| {
            !inventory_contains_device(batteries, configuration, &pending.device)
        }) {
            self.abandon();
        }
    }
    fn begin(
        &mut self,
        reading: ConfigurationDevice,
        intent: PollingIntent,
    ) -> Result<ControlRequest, &'static str> {
        self.begin_inner(reading, intent, false)
    }
    fn begin_tray(
        &mut self,
        reading: ConfigurationDevice,
        intent: PollingIntent,
    ) -> Result<ControlRequest, &'static str> {
        self.begin_inner(reading, intent, true)
    }
    fn begin_inner(
        &mut self,
        reading: ConfigurationDevice,
        intent: PollingIntent,
        tray: bool,
    ) -> Result<ControlRequest, &'static str> {
        if self.pending.is_some() {
            return Err("A device request is already pending");
        }
        if !reading.online() || !matches!(reading.capability, PollingCapability::ReadWrite) {
            return Err("Polling configuration is unavailable for this device");
        }
        let (target, action) = match intent {
            PollingIntent::Read => (
                ControlTarget {
                    device: reading,
                    generation: 0,
                },
                ControlAction::Read,
            ),
            PollingIntent::Apply { rate, restore } => {
                let observation = self.observations.get(&reading.key).filter(|o| {
                    o.target.device == reading && o.target.generation == self.generation
                });
                if let Some(observation) = observation {
                    if !observation.supported.contains(&rate)
                        || (observation.rate.is_none() && (!tray || restore))
                    {
                        return Err("Select a rate supported by this connection");
                    }
                    (observation.target.clone(), ControlAction::Apply(rate))
                } else if tray
                    && !restore
                    && hb_providers::controls::polling_menu_rates(&reading).contains(&rate.hz())
                {
                    // This is an explicit user selection, not an observed rate.
                    // Existing controllers resolve exact hardware capabilities,
                    // GET the current value, then SET and verify in one job.
                    (
                        ControlTarget {
                            device: reading,
                            generation: self.generation,
                        },
                        ControlAction::Apply(rate),
                    )
                } else {
                    return Err("Refresh the hardware rate and select a supported value");
                }
            }
        };
        self.sequence =
            (self.sequence.wrapping_add(1) & !crate::runtime::STARTUP_POLLING_REQUEST_BIT).max(1);
        self.pending = Some(PendingPolling {
            request: self.sequence,
            key: target.device.key.clone(),
            device: target.device.clone(),
            intent,
        });
        self.status.insert(
            target.device.key.clone(),
            match intent {
                PollingIntent::Read => "Reading hardware configuration…".into(),
                PollingIntent::Apply { restore: true, .. } => {
                    "Restoring the previous hardware rate…".into()
                }
                _ => "Applying and verifying the hardware rate…".into(),
            },
        );
        Ok(ControlRequest {
            request: self.sequence,
            target,
            action,
        })
    }
    fn accept(&mut self, outcome: &ControlOutcome, selected: Option<&str>) -> bool {
        let Some(pending) = self.pending.as_ref() else {
            return false;
        };
        if pending.request != outcome.request
            || pending.key != outcome.key
            || selected != Some(outcome.key.as_str())
        {
            return false;
        }
        let pending = self.pending.take().unwrap();
        if let Some(observation) = &outcome.observation {
            if observation.target.device != pending.device
                || observation.target.device.key != outcome.key
                || observation.target.generation < self.generation
            {
                self.observations.remove(&outcome.key);
                self.status.insert(
                    outcome.key.clone(),
                    "Device configuration changed; Refresh before applying".into(),
                );
                return true;
            }
            self.generation = observation.target.generation;
            self.observations
                .insert(outcome.key.clone(), observation.clone());
        } else {
            self.observations.remove(&outcome.key);
        }
        if let PollingIntent::Apply { restore: false, .. } = pending.intent
            && let Some(previous) = outcome.previous
        {
            // Keep the verified before-value for this explicit operation. A no-op
            // must not discard the recovery value from an earlier change.
            if let PollingIntent::Apply { rate, .. } = pending.intent
                && (rate != previous || outcome.failure.is_some())
            {
                self.previous.insert(outcome.key.clone(), previous);
            }
        }
        let observed = outcome.observation.as_ref().and_then(|o| o.rate);
        let message = if let Some(failure) = &outcome.failure {
            if outcome.may_have_changed {
                format!("Hardware may have changed. {failure} Refresh to verify.")
            } else {
                failure.clone()
            }
        } else {
            match pending.intent {
                PollingIntent::Read if observed.is_some() => {
                    "Hardware rate read. Changes require Apply.".into()
                }
                PollingIntent::Read => "Hardware did not report a polling rate.".into(),
                PollingIntent::Apply { rate, .. } if observed == Some(rate) => {
                    format!("Confirmed configured rate: {} Hz", rate.hz())
                }
                _ => "Change was not verified. Refresh the hardware rate before continuing.".into(),
            }
        };
        self.status.insert(outcome.key.clone(), message);
        true
    }
}
// UTC is stable between updates and does not imply a continuously refreshed age.
fn polling_timestamp(timestamp: i64) -> String {
    let days = timestamp.div_euclid(86_400);
    let seconds = timestamp.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}
struct HistorySelection {
    axis: HistoryAxis,
    usage_index: usize,
    calendar_index: usize,
}
impl Default for HistorySelection {
    fn default() -> Self {
        Self {
            axis: HistoryAxis::Usage,
            usage_index: 2,
            calendar_index: 0,
        }
    }
}
impl HistorySelection {
    fn index(&self) -> usize {
        match self.axis {
            HistoryAxis::Usage => self.usage_index,
            HistoryAxis::Calendar => self.calendar_index,
        }
    }
    fn set_index(&mut self, index: usize) {
        match self.axis {
            HistoryAxis::Usage => self.usage_index = index.min(2),
            HistoryAxis::Calendar => self.calendar_index = index.min(2),
        }
    }
    fn labels(&self) -> Vec<String> {
        match self.axis {
            HistoryAxis::Usage => ["2 hours used", "8 hours used", "24 hours used"],
            HistoryAxis::Calendar => ["24 hours", "7 days", "30 days"],
        }
        .into_iter()
        .map(str::to_owned)
        .collect()
    }
    fn description(&self) -> &'static str {
        match self.axis {
            HistoryAxis::Usage => {
                "Battery level: 0–100% · Estimated awake time; pauses sleeping, unavailable or charging"
            }
            HistoryAxis::Calendar => {
                "Battery level: 0–100% · Last known level held between readings"
            }
        }
    }
    fn command(&self, key: String, until: i64, width: usize, request: u64) -> Command {
        match self.axis {
            HistoryAxis::Usage => Command::UsageHistory {
                key,
                seconds: [2, 8, 24][self.index()] * 3600,
                until,
                width,
                request,
            },
            HistoryAxis::Calendar => Command::History {
                key,
                since: until - [1, 7, 30][self.index()] * 86400,
                until,
                width,
                request,
            },
        }
    }
}
#[derive(Default)]
struct InsightsUi {
    sequence: u64,
    pending: Option<(u64, String)>,
    key: Option<String>,
    data: Option<BatteryInsights>,
    status: String,
}
impl InsightsUi {
    fn abandon(&mut self) {
        self.pending = None;
    }
    fn select(&mut self, key: Option<String>) {
        if self.key != key {
            self.abandon();
            self.key = key;
            self.data = None;
            self.status.clear();
        }
    }
    fn accept(
        &mut self,
        request: u64,
        selected: Option<&str>,
        result: Result<BatteryInsights, ProviderError>,
    ) -> bool {
        if !self
            .pending
            .as_ref()
            .is_some_and(|(id, key)| *id == request && selected == Some(key.as_str()))
        {
            return false;
        }
        self.pending = None;
        match result {
            Ok(data) => {
                self.status = insight_coverage_text(&data);
                self.data = Some(data);
            }
            Err(error) => {
                self.status =
                    format!("Could not read local insights: {error}. Use Refresh to retry.")
            }
        }
        true
    }
}
const INSIGHTS_EMPTY: &str = "For rate comparisons, enable polling controls and confirm a supported device's rate using Refresh or a verified rate change in Devices or the tray menu. Saved requested rates are never evidence.";
fn insight_coverage_text(data: &BatteryInsights) -> String {
    let c = &data.coverage;
    let last = c
        .last_reading_timestamp
        .map(polling_timestamp)
        .unwrap_or_else(|| "none".into());
    let rate_time = data.rates.iter().map(|rate| rate.awake_seconds).sum();
    format!(
        "Local data refreshed. {} readings · {} discharging · {} counted use ({} with confirmed rate).\r\nLast stored reading: {last} · {} intervals excluded · {} unreadable rows.",
        c.observation_count,
        c.discharge_sample_count,
        insight_hours(c.awake_seconds),
        insight_hours(rate_time),
        c.excluded_interval_count,
        c.unreadable_row_count
    )
}
fn insight_empty_text(data: Option<&BatteryInsights>) -> String {
    let Some(data) = data else {
        return INSIGHTS_EMPTY.into();
    };
    let explanation = if data.coverage.observation_count == 0 {
        "No retained readings for this device yet."
    } else if data.coverage.discharge_sample_count == 0 {
        "No usable discharge readings yet. Charging, unavailable, imprecise or unknown-state readings cannot establish battery life."
    } else {
        "Discharge readings exist, but there is no continuous confirmed-rate usage to compare yet."
    };
    format!("{explanation}\r\n\r\n{INSIGHTS_EMPTY}")
}
fn insight_hours(seconds: u64) -> String {
    format!("{:.1} h", seconds as f64 / 3600.0)
}
fn insight_estimate(hours: Option<f64>) -> String {
    hours
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| {
            if value < 1.0 {
                "less than 1 h".into()
            } else {
                format!("about {value:.0} h")
            }
        })
        .unwrap_or_else(|| "Not enough discharge evidence".into())
}
fn rate_insight_text(rate: &RateInsight) -> String {
    let confidence = match rate.confidence {
        InsightConfidence::Insufficient => "Insufficient",
        InsightConfidence::Low => "Low",
        InsightConfidence::Moderate => "Moderate",
    };
    let guidance = match rate.confidence {
        InsightConfidence::Insufficient => {
            "Needs 30 min and 3 points between observed drops within continuous periods."
        }
        InsightConfidence::Low => {
            "Tentative: needs 2 h, 10 points and 3 complete drop intervals for moderate evidence."
        }
        InsightConfidence::Moderate => {
            "Observed average; usage conditions and battery rounding still affect the estimate."
        }
    };
    format!(
        "{} Hz · {} confidence\r\n{} awake · {} percentage points consumed\r\n{} samples · {} observed drops\r\nProjection evidence: {} · {} points · {} complete drop intervals\r\nEstimated full-charge use: {}\r\nRemaining at recent qualifying reading: {}\r\n{guidance}",
        rate.hz,
        confidence,
        insight_hours(rate.awake_seconds),
        rate.consumed_percent,
        rate.sample_count,
        rate.drop_count,
        insight_hours(rate.projection_seconds),
        rate.projection_consumed_percent,
        rate.projection_drop_count,
        insight_estimate(rate.projected_full_charge_hours),
        rate.remaining_hours
            .filter(|hours| hours.is_finite() && *hours >= 0.0)
            .map(|hours| insight_estimate(Some(hours)))
            .unwrap_or_else(|| "Unavailable (stale, paused or unconfirmed)".into())
    )
}
fn charge_cycle_row(cycle: &ChargeCycle) -> String {
    let timestamp = polling_timestamp(cycle.start_timestamp);
    format!(
        "{} · {}",
        &timestamp[5..16],
        insight_hours(cycle.awake_seconds)
    )
}
fn charge_cycle_text(cycle: &ChargeCycle) -> String {
    let evidence = match cycle.evidence {
        CycleEvidence::ObservedCharge => "Observed charge (does not imply a full charge)",
        CycleEvidence::InferredCharge => "Inferred charge from a battery rise",
        CycleEvidence::Partial => "Partial cycle; charge start was not observed",
    };
    let drain = if cycle.awake_seconds > 0 {
        format!(
            "{:.1} percentage points/h",
            cycle.consumed_percent as f64 * 3600.0 / cycle.awake_seconds as f64
        )
    } else {
        "Not enough awake evidence".into()
    };
    format!(
        "{} discharge summary · {}\r\n{} to {}\r\n{}% to {}% · {} percentage points consumed\r\n{} estimated awake time\r\nAverage observed drain: {}\r\nObserved segment, not a measured full-charge runtime.",
        if cycle.current { "Latest" } else { "Previous" },
        evidence,
        polling_timestamp(cycle.start_timestamp),
        polling_timestamp(cycle.end_timestamp),
        cycle.start_percent,
        cycle.end_percent,
        cycle.consumed_percent,
        insight_hours(cycle.awake_seconds),
        drain
    )
}
struct UiContext {
    state: RefCell<State>,
    monitor: Cell<HWND>,
    // Native child painting can reenter while State is being updated. Keep an
    // immutable, owned theme available without borrowing application state.
    paint: RefCell<Option<(HWND, Rc<DashboardTheme>)>>,
    popup: RefCell<Option<Rc<PopupAppearance>>>,
}
fn merged_device_rows(
    batteries: &[DeviceView],
    configuration: &[ConfigurationDevice],
) -> Vec<(ConfigurationDevice, Option<DeviceView>)> {
    let mut rows: Vec<_> = batteries
        .iter()
        .map(|d| {
            let device = configuration
                .iter()
                .find(|c| c.matches_reading(&d.reading))
                .cloned()
                .unwrap_or_else(|| ConfigurationDevice::from_reading(&d.reading));
            (device, Some(d.clone()))
        })
        .collect();
    for device in configuration {
        if !rows.iter().any(|(d, _)| d.key == device.key) {
            rows.push((device.clone(), None));
        }
    }
    rows
}
fn inventory_contains_device(
    batteries: &[DeviceView],
    configuration: &[ConfigurationDevice],
    device: &ConfigurationDevice,
) -> bool {
    if let Some(battery) = batteries.iter().find(|d| d.reading.key == device.key) {
        if let Some(configured) = configuration
            .iter()
            .find(|d| d.matches_reading(&battery.reading))
        {
            return configured == device;
        }
        let reading = &battery.reading;
        device.matches_reading(reading)
            && device.name == reading.name
            && device.kind == reading.kind
            && device.connection == reading.connection
            && matches!(device.capability, PollingCapability::ReadWrite)
    } else {
        configuration
            .iter()
            .find(|d| d.key == device.key)
            .is_some_and(|d| d == device)
    }
}
fn device_row_index(
    batteries: &[DeviceView],
    configuration: &[ConfigurationDevice],
    key: &str,
) -> Option<usize> {
    if let Some(index) = batteries.iter().position(|d| d.reading.key == key) {
        return Some(index);
    }
    let mut row = batteries.len();
    for (index, device) in configuration.iter().enumerate() {
        if batteries.iter().any(|d| d.reading.key == device.key)
            || configuration[..index].iter().any(|d| d.key == device.key)
        {
            continue;
        }
        if device.key == key {
            return Some(row);
        }
        row += 1;
    }
    None
}
fn current_device_row(
    batteries: &[DeviceView],
    configuration: &[ConfigurationDevice],
    selected: Option<&str>,
) -> Option<(ConfigurationDevice, Option<DeviceView>)> {
    let battery = selected.and_then(|key| batteries.iter().find(|d| d.reading.key == key));
    let standalone = selected.and_then(|key| configuration.iter().find(|d| d.key == key));
    if let Some(battery) = battery.or_else(|| {
        if standalone.is_none() {
            batteries.first()
        } else {
            None
        }
    }) {
        let device = configuration
            .iter()
            .find(|d| d.matches_reading(&battery.reading))
            .cloned()
            .unwrap_or_else(|| ConfigurationDevice::from_reading(&battery.reading));
        Some((device, Some(battery.clone())))
    } else {
        standalone
            .or_else(|| configuration.first())
            .cloned()
            .map(|d| (d, None))
    }
}
struct State {
    context: *const UiContext,
    runtime: Runtime,
    settings: Settings,
    dir: PathBuf,
    monitor: HWND,
    dashboard: Option<HWND>,
    controls: BTreeMap<u16, HWND>,
    trays: BTreeMap<String, Tray>,
    snapshot: Snapshot,
    diagnostics: BTreeMap<String, Vec<String>>,
    page: u16,
    selected: usize,
    selected_device: Option<String>,
    configuration_devices: Vec<ConfigurationDevice>,
    configuration_generation: u64,
    configuration_failure: Option<String>,
    chart: Option<Chart>,
    theme: Option<Rc<DashboardTheme>>,
    series: HistorySeries,
    history: HistorySelection,
    request: u64,
    taskbar: u32,
    notify: Option<HDEVNOTIFY>,
    animating: bool,
    tray_theme: TrayThemeCache,
    error: String,
    font: HFONT,
    polling: PollingUi,
    tray_polling: PollingUi,
    insights: InsightsUi,
    polling_intents: BTreeMap<String, (u64, PollingRate)>,
    tooltip_rates: BTreeMap<String, PollingObservation>,
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn copy(dst: &mut [u16], s: &str) {
    dst.fill(0);
    let limit = dst.len().saturating_sub(1);
    for (target, character) in dst.iter_mut().take(limit).zip(s.encode_utf16()) {
        *target = character;
    }
}
fn cache_tooltip_rate(
    cache: &mut BTreeMap<String, PollingObservation>,
    outcome: &ControlOutcome,
    generation: u64,
    enabled: bool,
) {
    cache.remove(&outcome.key);
    if enabled
        && outcome.failure.is_none()
        && let Some(observation) = &outcome.observation
        && observation.target.generation == generation
        && observation.target.device.key == outcome.key
        && observation.rate.is_some()
    {
        if cache.len() >= 512 {
            cache.pop_first();
        }
        cache.insert(outcome.key.clone(), observation.clone());
    }
}
/// Fixed Shell buffer; reserve the rate line even for a long renamed device.
/// Hover itself does no allocation, device I/O or timer work.
fn copy_tray_tooltip(
    dst: &mut [u16],
    device: &DeviceView,
    observation: Option<&PollingObservation>,
) {
    let rate = observation
        .filter(|o| {
            device.reading.connection != Connection::Stale
                && o.target.device.matches_reading(&device.reading)
        })
        .and_then(|o| o.rate);
    let suffix = match rate.map(|r| r.hz()) {
        Some(125) => "\nPolling: 125 Hz (last confirmed)",
        Some(250) => "\nPolling: 250 Hz (last confirmed)",
        Some(500) => "\nPolling: 500 Hz (last confirmed)",
        Some(1000) => "\nPolling: 1000 Hz (last confirmed)",
        Some(2000) => "\nPolling: 2000 Hz (last confirmed)",
        Some(4000) => "\nPolling: 4000 Hz (last confirmed)",
        Some(8000) => "\nPolling: 8000 Hz (last confirmed)",
        _ => "",
    };
    dst.fill(0);
    let available = dst.len().saturating_sub(1);
    let suffix_len = suffix.len().min(available);
    let prefix_limit = available - suffix_len;
    let mut position = 0;
    for character in device.text.chars() {
        let mut encoded = [0; 2];
        let units = character.encode_utf16(&mut encoded);
        if position + units.len() > prefix_limit {
            break;
        }
        dst[position..position + units.len()].copy_from_slice(units);
        position += units.len();
    }
    for unit in suffix.encode_utf16().take(suffix_len) {
        dst[position] = unit;
        position += 1;
    }
}

fn err(e: windows::core::Error) -> ProviderError {
    ProviderError::new(e.to_string())
}
pub fn run(
    runtime: Runtime,
    settings: Settings,
    dir: PathBuf,
    background: bool,
    initial_error: Option<String>,
) -> Result<(), ProviderError> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(None).map_err(err)?;
        let class = w!("HaloBatteryNext.Native");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(err)?,
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as usize as *mut _),
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return Err(ProviderError::new("Register window class failed"));
        }
        let font = CreateFontW(
            -16,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH.0 as u32,
            w!("Segoe UI"),
        );
        let context = Box::new(UiContext {
            state: RefCell::new(State {
                context: std::ptr::null(),
                runtime,
                settings,
                dir,
                monitor: HWND::default(),
                dashboard: None,
                controls: BTreeMap::new(),
                trays: BTreeMap::new(),
                snapshot: Snapshot::default(),
                diagnostics: BTreeMap::new(),
                page: 1,
                selected: 0,
                selected_device: None,
                configuration_devices: Vec::new(),
                configuration_generation: 0,
                configuration_failure: None,
                chart: None,
                theme: None,
                series: HistorySeries::default(),
                history: HistorySelection::default(),
                request: 0,
                taskbar: RegisterWindowMessageW(w!("TaskbarCreated")),
                notify: None,
                animating: false,
                tray_theme: TrayThemeCache::default(),
                error: initial_error.unwrap_or_default(),
                font,
                polling: PollingUi::default(),
                tray_polling: PollingUi::default(),
                insights: InsightsUi::default(),
                polling_intents: BTreeMap::new(),
                tooltip_rates: BTreeMap::new(),
            }),
            monitor: Cell::new(HWND::default()),
            paint: RefCell::new(None),
            popup: RefCell::new(None),
        });
        let ptr = &*context as *const UiContext;
        let mut state = context.state.borrow_mut();
        state.context = ptr;
        state.monitor = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("Halo Battery Next monitor"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            Some(ptr.cast()),
        )
        .map_err(err)?;
        context.monitor.set(state.monitor);
        // Consume a launcher's startup ShowWindow flag on the hidden monitor.
        let _ = ShowWindow(state.monitor, SW_HIDE);
        let filter = DEV_BROADCAST_DEVICEINTERFACE_W {
            dbcc_size: std::mem::size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32,
            dbcc_devicetype: DBT_DEVTYP_DEVICEINTERFACE.0,
            dbcc_classguid: HidD_GetHidGuid(),
            ..Default::default()
        };
        state.notify = RegisterDeviceNotificationW(
            HANDLE(state.monitor.0),
            (&filter as *const DEV_BROADCAST_DEVICEINTERFACE_W).cast(),
            DEVICE_NOTIFY_WINDOW_HANDLE,
        )
        .ok();
        state.runtime.attach_window(state.monitor.0 as usize);
        state.sync_trays(TrayUpdate::Changed);
        if !background {
            state.open();
        }
        // Runtime schedules initial discovery itself. Refresh here would invalidate
        // a startup restore that completed while the window was being created.
        drop(state);
        let mut message = MSG::default();
        loop {
            let result = GetMessageW(&mut message, None, 0, 0).0;
            if result <= 0 {
                break;
            }
            let dashboard = context.state.borrow().dashboard;
            if message.message == WM_SYSCHAR && b"dhsri".contains(&(message.wParam.0 as u8)) {
                DispatchMessageW(&message);
                continue;
            }
            if dashboard.is_some_and(|h| IsDialogMessageW(h, &message).as_bool()) {
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let mut state = context.state.borrow_mut();
        state.runtime.attach_window(0);
        state.trays.clear();
        if let Some(h) = state.dashboard.take() {
            let _ = DestroyWindow(h);
        }
        state.chart = None;
        if let Some(n) = state.notify.take() {
            let _ = UnregisterDeviceNotification(n);
        }
        let _ = DestroyWindow(state.monitor);
        let _ = DeleteObject(state.font.into());
        state.runtime.stop();
        let _ = UnregisterClassW(class, Some(instance.into()));
        Ok(())
    }
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        if msg == WM_NCCREATE {
            let cs = &*(lp.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const UiContext;
        if ptr.is_null() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        if let Some(result) = dashboard_paint_message(&*ptr, hwnd, msg, wp, lp) {
            return result;
        }
        if msg == WM_NCDESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            // Destruction is synchronous, including inside native modal loops.
            // Detach immediately, then release dashboard resources once the
            // outer State borrow ends. The hidden monitor outlives dashboards.
            if hwnd != (*ptr).monitor.get() {
                if let Ok(mut state) = (*ptr).state.try_borrow_mut() {
                    state.dashboard_destroyed(hwnd);
                } else {
                    let _ = PostMessageW(
                        Some((*ptr).monitor.get()),
                        WM_APP + 11,
                        WPARAM(hwnd.0 as usize),
                        LPARAM(0),
                    );
                }
            }
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        let Ok(mut guard) = (*ptr).state.try_borrow_mut() else {
            // DefWindowProc handles WM_CLOSE by destroying the window. Never
            // let that bypass State cleanup while a native callback reenters.
            let deferred = match msg {
                WM_CLOSE => Some((WM_APP + 9, WPARAM(hwnd.0 as usize))),
                m if m == WM_APP + 8 || m == WM_APP + 9 || m == WM_APP + 11 => Some((m, wp)),
                _ => None,
            };
            if let Some((message, param)) = deferred {
                let _ = PostMessageW(Some((*ptr).monitor.get()), message, param, lp);
                return LRESULT(0);
            }
            return DefWindowProcW(hwnd, msg, wp, lp);
        };
        let s = &mut *guard;
        if let Some(update) = tray_message_update(msg, s.taskbar) {
            if msg == WM_SETTINGCHANGE {
                s.refresh_theme();
            }
            s.sync_trays(update);
            return LRESULT(0);
        }
        match msg {
            WM_THEMECHANGED | WM_SYSCOLORCHANGE => {
                s.refresh_theme();
                s.sync_trays(TrayUpdate::Redraw);
                LRESULT(0)
            }
            m if m == WM_APP + 7 => {
                s.drain();
                LRESULT(0)
            }
            m if m == WM_APP + 8 => {
                s.open();
                LRESULT(0)
            }
            m if m == WM_APP + 9 => {
                if let Some(h) = s.dashboard
                    && (wp.0 == 0 || wp.0 == h.0 as usize)
                {
                    s.close_dashboard(h);
                }
                LRESULT(0)
            }
            m if m == WM_APP + 11 => {
                let destroyed = HWND(wp.0 as *mut _);
                // A recreated dashboard may reuse the old numeric HWND before
                // this deferred notification runs; only retire a stale owner.
                if !s.owns_dashboard(destroyed) {
                    s.dashboard_destroyed(destroyed);
                }
                LRESULT(0)
            }
            m if m == WM_APP + 10 => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_QUERYENDSESSION => LRESULT(1),
            WM_ENDSESSION => {
                if wp.0 != 0 {
                    s.runtime.stop();
                    PostQuitMessage(0);
                }
                LRESULT(0)
            }
            WM_TIMER => {
                if wp.0 == 2 {
                    s.animate()
                } else {
                    s.drain()
                }
                LRESULT(0)
            }
            WM_DEVICECHANGE => {
                s.runtime.send(Command::Refresh);
                LRESULT(1)
            }
            WM_POWERBROADCAST => {
                match wp.0 as u32 {
                    PBT_APMSUSPEND => s.runtime.send(Command::Suspend),
                    PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => {
                        s.runtime.send(Command::Resume)
                    }
                    _ => {}
                }
                LRESULT(1)
            }
            TRAY => {
                let key = s
                    .trays
                    .iter()
                    .find(|(_, t)| t.data.uID == wp.0 as u32)
                    .map(|(key, _)| key.clone());
                if let Some(key) = &key
                    && let Some(i) = s
                        .snapshot
                        .devices
                        .iter()
                        .position(|d| &d.reading.key == key)
                {
                    s.selected_device = Some(key.clone());
                    if s.selected != i && s.page == 6 {
                        s.selected = i;
                        s.insights.abandon();
                        s.build();
                        s.query_insights();
                    } else {
                        s.selected = i;
                    }
                }
                let event = (lp.0 as u32) & 0xffff;
                if event == WM_CONTEXTMENU || event == WM_RBUTTONUP {
                    s.menu(key.clone())
                } else if event == WM_LBUTTONUP
                    || event == WM_LBUTTONDBLCLK
                    || event == NIN_SELECT
                    || event == (NIN_SELECT | 1)
                {
                    s.open()
                }
                LRESULT(0)
            }
            WM_SYSCHAR => {
                match (wp.0 as u8).to_ascii_lowercase() {
                    b'd' => s.command(1, 0),
                    b'h' => s.command(2, 0),
                    b's' => s.command(3, 0),
                    b'i' => s.command(6, 0),
                    b'r' => s.command(4, 0),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                s.command((wp.0 & 0xffff) as u16, ((wp.0 >> 16) & 0xffff) as u16);
                LRESULT(0)
            }
            WM_CLOSE => {
                if Some(hwnd) == s.dashboard {
                    s.close_dashboard(hwnd);
                }
                LRESULT(0)
            }
            WM_GETMINMAXINFO => {
                let m = &mut *(lp.0 as *mut MINMAXINFO);
                let dpi = GetDpiForWindow(hwnd).max(96) as i32;
                m.ptMinTrackSize = POINT {
                    x: 840 * dpi / 96,
                    y: 820 * dpi / 96,
                };
                LRESULT(0)
            }
            WM_SIZE => {
                if Some(hwnd) == s.dashboard && s.page == 2 {
                    s.query();
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let r = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                s.build();
                s.sync_trays(TrayUpdate::Redraw);
                LRESULT(0)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                // BeginPaint can synchronously request background erasure while
                // State is borrowed. Paint the palette ourselves rather than
                // leaving the class's default system brush on screen.
                if Some(hwnd) == s.dashboard
                    && let Some(theme) = &s.theme
                {
                    FillRect(hdc, &ps.rcPaint, theme.background_brush());
                }
                if Some(hwnd) == s.dashboard && s.page == 2 {
                    let mut r = RECT::default();
                    let _ = GetClientRect(hwnd, &mut r);
                    if s.chart.is_none() {
                        s.chart = Chart::new(hwnd, r.right as u32, r.bottom as u32).ok()
                    }
                    if let Some(c) = &s.chart
                        && s.theme.as_ref().is_some_and(|theme| {
                            c.paint_with_palette(
                                r.right as u32,
                                r.bottom as u32,
                                &s.series,
                                &theme.palette,
                            )
                            .is_err()
                        })
                    {
                        s.chart = None;
                    }
                }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            _ => {
                // Native default processing can synchronously send messages
                // back to this window (SC_CLOSE, WM_PRINT, activation). Release
                // State before handing control back to Windows.
                drop(guard);
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
    }))
    .unwrap_or_else(|_| {
        unsafe {
            PostQuitMessage(1);
        }
        LRESULT(0)
    })
}
unsafe fn dashboard_paint_message(
    context: &UiContext,
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> Option<LRESULT> {
    if hwnd == context.monitor.get() && matches!(msg, WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR) {
        // TrackPopupMenu reenters while State is borrowed; retain only the
        // immutable menu snapshot and release the context borrow before GDI.
        let appearance = context.popup.borrow().clone();
        if let Some(appearance) = appearance {
            return appearance.message(msg, wp, lp);
        }
    }
    if !matches!(
        msg,
        WM_DRAWITEM
            | WM_CTLCOLOREDIT
            | WM_CTLCOLORLISTBOX
            | WM_CTLCOLORSTATIC
            | WM_CTLCOLORBTN
            | WM_ERASEBKGND
            | WM_PRINTCLIENT
    ) {
        return None;
    }
    // Drop the RefCell borrow before calling Windows; owner drawing can itself
    // send synchronous native messages. Rc retains the brushes through a call.
    let (owner, theme) = context.paint.borrow().as_ref()?.clone();
    if hwnd != owner {
        return None;
    }
    if let Some(result) = theme.control_colors(msg, HDC(wp.0 as *mut _), HWND(lp.0 as *mut _)) {
        return Some(result);
    }
    if msg == WM_DRAWITEM {
        return theme.draw_item(lp);
    }
    if matches!(msg, WM_ERASEBKGND | WM_PRINTCLIENT) {
        unsafe {
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            FillRect(HDC(wp.0 as *mut _), &rect, theme.background_brush());
        }
        return Some(LRESULT(i32::from(msg == WM_ERASEBKGND) as isize));
    }
    None
}

/// Hide intermediate child teardown/creation from the display. An initially
/// hidden dashboard must remain hidden until open() explicitly shows it.
struct DashboardRedraw {
    hwnd: HWND,
    paused: bool,
}
impl DashboardRedraw {
    fn new(hwnd: HWND) -> Self {
        let paused = unsafe { IsWindowVisible(hwnd).as_bool() };
        if paused {
            unsafe {
                send(hwnd, WM_SETREDRAW, WPARAM(0), LPARAM(0));
            }
        }
        Self { hwnd, paused }
    }
}
impl Drop for DashboardRedraw {
    fn drop(&mut self) {
        unsafe {
            if self.paused {
                send(self.hwnd, WM_SETREDRAW, WPARAM(1), LPARAM(0));
            }
            let _ = RedrawWindow(
                Some(self.hwnd),
                None,
                None,
                RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN,
            );
        }
    }
}
impl State {
    fn refresh_theme(&mut self) {
        if self.dashboard.is_none() {
            return;
        }
        let dark = hb_windows::system::dashboard_dark_theme();
        let high_contrast = hb_windows::system::high_contrast();
        let palette = crate::dashboard_theme::Palette::new(dark, high_contrast);
        if self
            .theme
            .as_ref()
            .is_some_and(|theme| theme.palette == palette)
        {
            return;
        }
        self.apply_theme(DashboardTheme::new(dark, high_contrast));
    }
    fn apply_theme(&mut self, theme: DashboardTheme) {
        let Some(hwnd) = self.dashboard else {
            return;
        };
        let _redraw = DashboardRedraw::new(hwnd);
        let theme = Rc::new(theme);
        unsafe {
            (*self.context).paint.replace(Some((hwnd, theme.clone())));
        }
        self.theme = Some(theme.clone());
        theme.apply_window(hwnd);
        for control in self.controls.values() {
            theme.apply_control(*control);
        }
    }
    fn owns_dashboard(&self, hwnd: HWND) -> bool {
        unsafe {
            IsWindow(Some(hwnd)).as_bool()
                && GetWindowLongPtrW(hwnd, GWLP_USERDATA) == self.context as isize
        }
    }
    fn dashboard_destroyed(&mut self, hwnd: HWND) {
        if self.dashboard != Some(hwnd) {
            return;
        }
        self.dashboard = None;
        self.configuration_visibility();
        self.chart = None;
        self.release_history();
        self.theme = None;
        unsafe {
            (*self.context).paint.take();
        }
        self.polling.abandon();
        self.insights.abandon();
        self.controls.clear();
        unsafe {
            let _ = DeleteObject(self.font.into());
        }
        self.font = HFONT::default();
    }
    fn close_dashboard(&mut self, hwnd: HWND) {
        if self.dashboard != Some(hwnd) {
            return;
        }
        // Keep GDI/control resources alive until native children are destroyed.
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        self.dashboard_destroyed(hwnd);
    }

    #[allow(clippy::too_many_arguments)] // Mirrors the native control creation fields.
    fn control(
        &mut self,
        id: u16,
        class: PCWSTR,
        text: &str,
        style: WINDOW_STYLE,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> HWND {
        unsafe {
            let text = wide(text);
            let scale = GetDpiForWindow(self.dashboard.unwrap()) as i32;
            let f = |n: i32| n * scale / 96;
            let h = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                PCWSTR(text.as_ptr()),
                WS_CHILD | WS_VISIBLE | style,
                f(x),
                f(y),
                f(width),
                f(height),
                self.dashboard,
                Some(HMENU(id as usize as *mut _)),
                None,
                None,
            )
            .unwrap_or_default();
            send(h, WM_SETFONT, WPARAM(self.font.0 as usize), LPARAM(1));
            if let Some(theme) = &self.theme {
                theme.apply_control(h);
            }
            self.controls.insert(id, h);
            h
        }
    }
    fn label(&mut self, id: u16, text: &str, x: i32, y: i32, width: i32) {
        self.control(
            id,
            w!("STATIC"),
            text,
            WINDOW_STYLE::default(),
            x,
            y,
            width,
            24,
        );
    }
    fn button(&mut self, id: u16, text: &str, x: i32, y: i32, width: i32) {
        self.control(id, w!("BUTTON"), text, WS_TABSTOP, x, y, width, 30);
    }
    fn edit(&mut self, id: u16, text: &str, x: i32, y: i32, width: i32) {
        self.control(
            id,
            w!("EDIT"),
            text,
            WS_TABSTOP | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            x,
            y,
            width,
            26,
        );
    }
    fn check(&mut self, id: u16, text: &str, value: bool, x: i32, y: i32, width: i32) {
        let h = self.control(
            id,
            w!("BUTTON"),
            text,
            WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
            x,
            y,
            width,
            26,
        );
        unsafe {
            send(h, BM_SETCHECK, WPARAM(value as usize), LPARAM(0));
        }
    }
    fn combo(&mut self, id: u16, items: &[String], selected: usize, x: i32, y: i32, width: i32) {
        let h = self.control(
            id,
            w!("COMBOBOX"),
            "",
            WS_TABSTOP
                | WS_VSCROLL
                | WINDOW_STYLE((CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS) as u32),
            x,
            y,
            width,
            220,
        );
        unsafe {
            for t in items {
                let text = wide(t);
                send(h, CB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize));
            }
            send(h, CB_SETCURSEL, WPARAM(selected), LPARAM(0));
        }
    }
    fn text(&self, id: u16) -> String {
        unsafe {
            let Some(h) = self.controls.get(&id) else {
                return String::new();
            };
            let n = GetWindowTextLengthW(*h) as usize;
            let mut b = vec![0; n + 1];
            GetWindowTextW(*h, &mut b);
            String::from_utf16_lossy(&b[..n])
        }
    }
    fn checked(&self, id: u16) -> bool {
        self.controls
            .get(&id)
            .is_some_and(|h| unsafe { send(*h, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1 })
    }
    fn choice(&self, id: u16) -> usize {
        self.controls.get(&id).map_or(0, |h| unsafe {
            send(*h, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0.max(0) as usize
        })
    }
    fn open(&mut self) {
        unsafe {
            if let Some(h) = self.dashboard {
                if self.owns_dashboard(h) {
                    let _ = ShowWindow(h, SW_RESTORE);
                    let _ = SetForegroundWindow(h);
                    return;
                }
                self.dashboard_destroyed(h);
            }
            let ptr = self.context;
            let dpi = GetDpiForWindow(self.monitor).max(96) as i32;
            match CreateWindowExW(
                WS_EX_CONTROLPARENT,
                w!("HaloBatteryNext.Native"),
                w!("Halo Battery Next"),
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                840 * dpi / 96,
                820 * dpi / 96,
                None,
                None,
                None,
                Some(ptr.cast()),
            ) {
                Ok(h) => {
                    self.dashboard = Some(h);
                    self.configuration_visibility();
                    self.refresh_theme();
                    self.polling.abandon();
                    self.build();
                    self.read_polling();
                    self.query_insights();
                    let _ = ShowWindow(h, SW_SHOW);
                    let _ = SetForegroundWindow(h);
                }
                Err(e) => self.error = e.to_string(),
            }
        }
    }
    fn build(&mut self) {
        let Some(hwnd) = self.dashboard else {
            return;
        };
        let _redraw = DashboardRedraw::new(hwnd);
        self.chart = None;
        if self.page != 2 {
            self.release_history();
        }
        let controls = std::mem::take(&mut self.controls);
        unsafe {
            for h in controls.values() {
                let _ = DestroyWindow(*h);
            }
        }
        unsafe {
            let dpi = GetDpiForWindow(self.dashboard.unwrap()) as i32;
            let old = self.font;
            self.font = CreateFontW(
                -16 * dpi / 96,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                DEFAULT_PITCH.0 as u32,
                w!("Segoe UI"),
            );
            let _ = DeleteObject(old.into());
        }
        self.button(1, "&Devices", 20, 16, 110);
        self.button(2, "&History", 140, 16, 110);
        self.button(3, "&Settings", 260, 16, 110);
        self.button(6, "&Insights", 380, 16, 110);
        self.button(4, "&Refresh", 660, 16, 120);
        let names: Vec<_> = self
            .snapshot
            .devices
            .iter()
            .map(|d| d.text.clone())
            .collect();
        self.selected = self.selected.min(names.len().saturating_sub(1));
        match self.page {
            1 => {
                self.label(
                    90,
                    "Select a device to customize its name and available settings",
                    20,
                    64,
                    740,
                );
                let rows = self.device_rows();
                let index = self
                    .selected_device
                    .as_ref()
                    .and_then(|key| rows.iter().position(|(d, _)| &d.key == key))
                    .unwrap_or(0);
                self.selected_device = rows.get(index).map(|(d, _)| d.key.clone());
                let device_names = rows
                    .iter()
                    .map(|(d, battery)| {
                        battery.as_ref().map_or_else(
                            || {
                                format!(
                                    "{} · Wired keyboard · No battery",
                                    self.configuration_name(d)
                                )
                            },
                            |b| b.text.clone(),
                        )
                    })
                    .collect::<Vec<_>>();
                self.combo(10, &device_names, index, 20, 96, 760);
                if let Some((device, battery)) = self.current_device() {
                    if let Some(d) = battery {
                        self.label(91, &device_detail(&d), 20, 138, 740);
                        self.label(92, "&Name", 20, 184, 120);
                        self.edit(11, &d.name, 150, 180, 360);
                        self.check(12, "&Hide tray icon", d.hidden, 20, 222, 350);
                        self.label(93, "Low alert % (blank = default)", 20, 270, 230);
                        let low = self
                            .settings
                            .devices
                            .get(&d.reading.key)
                            .and_then(|p| p.low)
                            .map(|v| v.to_string())
                            .unwrap_or_default();
                        self.edit(13, &low, 260, 266, 80);
                        self.label(94, "Tray &icon", 20, 310, 120);
                        let kinds: Vec<_> = [
                            "automatic",
                            "mouse",
                            "keyboard",
                            "headset",
                            "gamepad",
                            "bluetooth",
                            "dualshock",
                            "dualsense",
                        ]
                        .iter()
                        .map(|s| s.to_string())
                        .collect();
                        let chosen = self
                            .settings
                            .devices
                            .get(&d.reading.key)
                            .and_then(|p| p.icon.as_ref())
                            .and_then(|icon| kinds.iter().position(|s| s == icon))
                            .unwrap_or(0);
                        self.combo(14, &kinds, chosen, 150, 306, 240);
                        self.button(15, "&Save device", 20, 360, 160);
                        self.button(16, "Reset to defaults", 190, 360, 160);
                    } else {
                        self.label(91, "Wired keyboard · No battery", 20, 138, 740);
                        self.label(92, "&Name", 20, 184, 120);
                        self.edit(11, &self.configuration_name(&device), 150, 180, 360);
                        self.button(15, "&Save device", 20, 360, 160);
                        self.button(16, "Reset name", 190, 360, 160);
                    }
                } else {
                    self.label(
                        91,
                        "No devices detected yet. Use Refresh or review provider diagnostics.",
                        20,
                        150,
                        740,
                    );
                }
                self.polling_controls();
                self.label(95, &self.error.clone(), 20, 680, 750);
                self.button(5, "Export &diagnostics", 20, 725, 200);
            }
            2 => {
                self.combo(10, &names, self.selected, 20, 65, 380);
                self.combo(
                    21,
                    &["Time used".into(), "Calendar time".into()],
                    usize::from(self.history.axis == HistoryAxis::Calendar),
                    420,
                    65,
                    160,
                );
                self.combo(
                    20,
                    &self.history.labels(),
                    self.history.index(),
                    600,
                    65,
                    180,
                );
                self.label(96, self.history.description(), 20, 105, 750);
                self.label(
                    97,
                    "30-day retention · Estimated device awake time · No input tracking",
                    20,
                    735,
                    760,
                );
                self.query();
            }
            3 => {
                let value = serde_json::to_value(&self.settings).unwrap_or_default();
                for (i, (key, label)) in CHECKS.iter().enumerate() {
                    self.check(
                        100 + i as u16,
                        label,
                        value[*key].as_bool().unwrap_or(false),
                        20 + (i as i32 / 6) * 390,
                        65 + (i as i32 % 6) * 32,
                        385,
                    );
                }
                if let Some(h) = self.controls.get(&111) {
                    unsafe {
                        SetWindowLongW(
                            *h,
                            GWL_STYLE,
                            GetWindowLongW(*h, GWL_STYLE) | WS_DISABLED.0 as i32,
                        );
                    }
                }
                self.check(
                    112,
                    "Enable polling-rate controls",
                    self.settings.polling_controls,
                    20,
                    258,
                    365,
                );
                self.check(
                    113,
                    "Restore saved rates at startup",
                    self.settings.restore_polling_on_startup,
                    410,
                    258,
                    370,
                );
                self.label(200, "Battery refresh interval", 20, 302, 230);
                self.edit(201, &self.settings.interval.to_string(), 260, 298, 100);
                self.label(202, "Low alert %", 390, 302, 95);
                self.edit(203, &self.settings.low.to_string(), 490, 298, 55);
                self.label(214, "Orange warning %", 560, 302, 150);
                self.edit(215, &self.settings.warning_level.to_string(), 720, 298, 55);
                let themes: Vec<_> = ["auto", "white", "black", "windows", "topbar"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                self.label(204, "Icon theme", 20, 342, 150);
                self.combo(
                    205,
                    &themes,
                    themes
                        .iter()
                        .position(|s| s == &self.settings.icon_theme)
                        .unwrap_or(0),
                    180,
                    338,
                    180,
                );
                self.check(
                    206,
                    "Launch at sign in",
                    hb_windows::system::is_startup(),
                    410,
                    338,
                    360,
                );
                self.label(207, "Provider switches", 20, 382, 740);
                for (i, p) in providers().iter().enumerate() {
                    self.check(
                        300 + i as u16,
                        provider_label(p),
                        self.settings.enabled(p),
                        20 + (i as i32 % 4) * 190,
                        412 + (i as i32 / 4) * 30,
                        180,
                    );
                }
                self.label(208, "Release repository (owner/name)", 20, 636, 300);
                self.edit(
                    209,
                    self.settings
                        .release_repository
                        .clone()
                        .as_deref()
                        .unwrap_or(""),
                    330,
                    632,
                    410,
                );
                self.button(210, "&Save settings", 20, 682, 180);
                self.button(5, "Export &diagnostics", 220, 682, 200);
                self.label(95, &self.error.clone(), 20, 732, 750);
            }
            6 => self.build_insights(&names),
            _ => {}
        }
    }
    fn set_control_text(&self, id: u16, text: &str) {
        if let Some(h) = self.controls.get(&id) {
            if self.text(id) == text {
                return;
            }
            let text = wide(text);
            unsafe {
                let _ = SetWindowTextW(*h, PCWSTR(text.as_ptr()));
            }
        }
    }
    fn insights_edit(&mut self, id: u16, y: i32, height: i32) {
        self.control(
            id,
            w!("EDIT"),
            "",
            WS_TABSTOP
                | WS_BORDER
                | WS_VSCROLL
                | WINDOW_STYLE((ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32),
            225,
            y,
            555,
            height,
        );
    }
    fn insights_list(&mut self, id: u16, y: i32, height: i32) {
        self.control(
            id,
            w!("LISTBOX"),
            "",
            WS_TABSTOP
                | WS_BORDER
                | WS_VSCROLL
                | WINDOW_STYLE((LBS_NOTIFY | LBS_NOINTEGRALHEIGHT) as u32),
            20,
            y,
            190,
            height,
        );
    }
    fn build_insights(&mut self, names: &[String]) {
        let key = self
            .snapshot
            .devices
            .get(self.selected)
            .map(|d| d.reading.key.clone());
        self.insights.select(key);
        self.combo(10, names, self.selected, 20, 65, 760);
        self.label(
            72,
            "Battery Insights · local 30-day data · Refresh updates this page",
            20,
            105,
            760,
        );
        self.label(
            73,
            "Polling-rate comparison · estimated full-charge awake runtime",
            20,
            140,
            760,
        );
        self.insights_list(70, 170, 155);
        self.insights_edit(74, 170, 155);
        self.label(
            75,
            "Recent charge summaries · newest first · UTC start time (up to 10)",
            20,
            340,
            760,
        );
        self.insights_list(71, 370, 165);
        self.insights_edit(76, 370, 165);
        self.control(77, w!("STATIC"),
            "Awake use measures observed device availability, not input activity. Rate comparisons average up to 30 days; the tray estimate follows recent discharge and can differ. Neither measures battery health.\r\n\r\nRates use the last confirmed setting. Confirm again after changing it elsewhere. Sleep, charging, missing readings and connection boundaries are excluded from learning. No hardware polling is added by this page.",
            WINDOW_STYLE::default(), 20, 550, 760, 140);
        self.control(
            78,
            w!("STATIC"),
            "",
            WINDOW_STYLE::default(),
            20,
            700,
            760,
            60,
        );
        self.render_insights();
    }
    fn query_insights(&mut self) {
        if self.dashboard.is_none() || self.page != 6 {
            return;
        }
        let key = self
            .snapshot
            .devices
            .get(self.selected)
            .map(|d| d.reading.key.clone());
        self.insights.select(key.clone());
        let Some(key) = key else {
            self.insights.status =
                "No device selected. Connect a device and use Refresh under Devices.".into();
            self.render_insights();
            return;
        };
        self.insights.sequence = self.insights.sequence.wrapping_add(1).max(1);
        let request = self.insights.sequence;
        self.insights.pending = Some((request, key.clone()));
        self.insights.status = "Reading local discharge evidence…".into();
        self.set_control_text(78, &self.insights.status);
        self.runtime.send(Command::Insights {
            key,
            until: SystemClock::default().unix(),
            request,
        });
    }
    fn render_insights(&self) {
        if self.dashboard.is_none() || self.page != 6 {
            return;
        }
        for id in [70, 71] {
            if let Some(h) = self.controls.get(&id) {
                unsafe {
                    send(*h, LB_RESETCONTENT, WPARAM(0), LPARAM(0));
                }
            }
        }
        if let Some(data) = &self.insights.data {
            for (id, rows) in [
                (
                    70,
                    data.rates
                        .iter()
                        .map(|rate| {
                            let estimate = rate
                                .projected_full_charge_hours
                                .filter(|hours| hours.is_finite() && *hours >= 0.0)
                                .map(|hours| format!("~{hours:.0} h"))
                                .unwrap_or_else(|| "limited data".into());
                            format!("{} Hz · {estimate}", rate.hz)
                        })
                        .collect::<Vec<_>>(),
                ),
                (
                    71,
                    data.cycles
                        .iter()
                        .rev()
                        .take(10)
                        .map(charge_cycle_row)
                        .collect(),
                ),
            ] {
                if let Some(h) = self.controls.get(&id) {
                    for row in rows {
                        let text = wide(&row);
                        unsafe {
                            send(*h, LB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize));
                        }
                    }
                    unsafe {
                        send(*h, LB_SETCURSEL, WPARAM(0), LPARAM(0));
                    }
                }
            }
        }
        self.insights_details();
        self.set_control_text(78, &self.insights.status);
    }
    fn insights_details(&self) {
        if self.dashboard.is_none() || self.page != 6 {
            return;
        }
        let selected = |id| {
            self.controls.get(&id).map_or(0, |h| unsafe {
                send(*h, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0.max(0) as usize
            })
        };
        let rate = self
            .insights
            .data
            .as_ref()
            .and_then(|data| data.rates.get(selected(70)))
            .map(rate_insight_text)
            .unwrap_or_else(|| insight_empty_text(self.insights.data.as_ref()));
        let cycle = self.insights.data.as_ref().and_then(|data| data.cycles.iter().rev().nth(selected(71)))
            .map(charge_cycle_text).unwrap_or_else(|| "No charge summaries yet. Collect awake discharge readings; partial cycles appear when sufficient connected data is available.".into());
        self.set_control_text(74, &rate);
        self.set_control_text(76, &cycle);
    }
    fn save(&mut self) {
        self.runtime.send(Command::Settings(self.settings.clone()));
        self.sync_trays(TrayUpdate::Redraw);
    }
    fn command(&mut self, id: u16, notification: u16) {
        match id {
            1..=3 | 6 => {
                self.polling.abandon();
                self.insights.abandon();
                self.page = id;
                self.configuration_visibility();
                self.build();
                self.read_polling();
                self.query_insights()
            }
            4 => {
                if self.page == 6 {
                    self.query_insights();
                    return;
                }
                self.runtime.send(Command::Refresh);
                if self.page == 1 {
                    self.runtime.send(Command::ConfigurationRefresh);
                    self.read_polling();
                }
                if self.page == 2 {
                    self.query()
                }
            }
            5 => {
                let path = self.dir.join("diagnostics.json");
                let value = serde_json::json!({"snapshot":self.snapshot,"providers":self.diagnostics,"settings":self.settings,"application_error":self.error});
                self.error = match hb_storage::atomic_write(
                    &path,
                    &serde_json::to_vec_pretty(&value).unwrap(),
                ) {
                    Ok(()) => format!("Exported {}", path.display()),
                    Err(e) => e.to_string(),
                };
                self.build()
            }
            10 if notification == CBN_SELCHANGE as u16 => {
                self.polling.abandon();
                self.insights.abandon();
                if self.page == 1 {
                    self.selected_device = self
                        .device_rows()
                        .get(self.choice(10))
                        .map(|(d, _)| d.key.clone());
                    if let Some(index) = self
                        .snapshot
                        .devices
                        .iter()
                        .position(|d| Some(&d.reading.key) == self.selected_device.as_ref())
                    {
                        self.selected = index;
                    }
                } else {
                    self.selected = self.choice(10);
                }
                self.build();
                self.read_polling();
                self.query_insights()
            }
            70 | 71 if notification == LBN_SELCHANGE as u16 => self.insights_details(),
            40 => self.apply_polling(false),
            41 => self.read_polling(),
            42 => self.apply_polling(true),
            20 if notification == CBN_SELCHANGE as u16 => {
                self.history.set_index(self.choice(20));
                self.build()
            }
            21 if notification == CBN_SELCHANGE as u16 => {
                self.history.axis = if self.choice(21) == 0 {
                    HistoryAxis::Usage
                } else {
                    HistoryAxis::Calendar
                };
                self.build()
            }
            15 => {
                if let Some((device, battery)) = self.current_device()
                    && battery.is_none()
                {
                    let name = self.text(11);
                    self.settings.devices.entry(device.key).or_default().name =
                        (!name.trim().is_empty()).then_some(name);
                    self.save();
                    self.build();
                    return;
                }
                if let Some(d) = self.current_device().and_then(|(_, battery)| battery) {
                    let key = d.reading.key.clone();
                    let name = self.text(11);
                    let hidden = self.checked(12);
                    let low = match device_threshold(&self.text(13)) {
                        Ok(low) => low,
                        Err(e) => {
                            self.error = e.into();
                            self.build();
                            return;
                        }
                    };
                    let choice = self.choice(14);
                    let icon = if choice == 0 {
                        None
                    } else {
                        Some(
                            [
                                "mouse",
                                "keyboard",
                                "headset",
                                "gamepad",
                                "bluetooth",
                                "dualshock",
                                "dualsense",
                            ][(choice - 1).min(6)]
                            .to_string(),
                        )
                    };
                    self.settings.devices.insert(
                        key.clone(),
                        DevicePreferences {
                            name: (!name.trim().is_empty()).then_some(name),
                            hidden,
                            icon,
                            low,
                            requested_polling_rate: self
                                .settings
                                .devices
                                .get(&key)
                                .and_then(|p| p.requested_polling_rate),
                        },
                    );
                    self.save();
                }
            }
            16 => {
                if let Some((device, battery)) = self.current_device() {
                    if battery.is_some() {
                        self.settings.devices.remove(&device.key);
                    } else if let Some(preferences) = self.settings.devices.get_mut(&device.key) {
                        preferences.name = None;
                    }
                    self.save();
                    self.build();
                }
            }
            210 => {
                let mut value = serde_json::to_value(&self.settings).unwrap();
                for (i, (key, _)) in CHECKS.iter().enumerate() {
                    value[*key] = self.checked(100 + i as u16).into()
                }
                let interval = self
                    .text(201)
                    .parse::<u64>()
                    .ok()
                    .filter(|n| (5..=3600).contains(n));
                let low = self.text(203).parse::<u8>().ok().filter(|n| *n <= 100);
                let warning = self.text(215).parse::<u8>().ok().filter(|n| *n <= 100);
                if interval.is_none() || low.is_none() || warning.is_none() {
                    self.error =
                        "Battery refresh interval must be 5–3600 seconds; alert must be 0–100"
                            .into();
                    self.build();
                    return;
                }
                value["polling_controls"] = self.checked(112).into();
                value["restore_polling_on_startup"] = self.checked(113).into();
                value["interval"] = interval.unwrap().into();
                value["low"] = low.unwrap().into();
                value["warning_level"] = warning.unwrap().into();
                value["icon_theme"] =
                    ["auto", "white", "black", "windows", "topbar"][self.choice(205).min(4)].into();
                let repo = self.text(209);
                if !repo.is_empty() && !hb_core::settings::valid_repository(&repo) {
                    self.error = "Use owner/name for the release repository".into();
                    self.build();
                    return;
                }
                value["release_repository"] = if repo.is_empty() {
                    serde_json::Value::Null
                } else {
                    repo.into()
                };
                self.settings = Settings::from_value(value).unwrap();
                self.settings.disabled_providers = providers()
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !self.checked(300 + *i as u16))
                    .map(|(_, p)| p.to_string())
                    .collect();
                self.error = match hb_windows::system::startup(self.checked(206)) {
                    Ok(()) => "Settings saved".into(),
                    Err(e) => e.to_string(),
                };
                self.save();
                self.polling.abandon();
                if !self.settings.polling_controls {
                    self.tooltip_rates.clear();
                    self.polling.observations.clear();
                    self.tray_polling.invalidate(self.tray_polling.generation);
                }
                self.build()
            }
            503 => {
                self.page = 1;
                self.configuration_visibility();
                self.open();
                self.build();
            }
            504 => {
                self.page = 2;
                self.configuration_visibility();
                self.open();
                self.build();
            }
            505 => {
                if let Some(d) = self.snapshot.devices.get(self.selected) {
                    self.settings
                        .devices
                        .entry(d.reading.key.clone())
                        .or_default()
                        .hidden = true;
                    self.save();
                }
            }
            600..=999 => {
                self.selected = (id - 600) as usize;
                self.selected_device = self
                    .snapshot
                    .devices
                    .get(self.selected)
                    .map(|d| d.reading.key.clone());
                self.page = 1;
                self.configuration_visibility();
                self.open();
                self.build();
            }
            500 => self.open(),
            501 => self.runtime.send(Command::Refresh),
            502 => unsafe { PostQuitMessage(0) },
            _ => {}
        }
    }
    fn configuration_inventory(
        &mut self,
        generation: u64,
        devices: Vec<ConfigurationDevice>,
        failure: Option<String>,
    ) {
        if generation < self.configuration_generation {
            return;
        }
        self.configuration_generation = generation;
        let failure_changed = failure != self.configuration_failure;
        if let Some(error) = &failure {
            self.error = error.clone();
        } else if self
            .configuration_failure
            .as_ref()
            .is_some_and(|previous| &self.error == previous)
        {
            self.error.clear();
        }
        self.configuration_failure = failure;
        if devices == self.configuration_devices {
            if failure_changed && self.dashboard.is_some() && self.page == 1 {
                self.set_control_text(95, &self.error.clone());
            }
            return;
        }
        let previous = self.current_device().map(|(d, _)| d);
        self.configuration_devices = devices;
        let current = self.current_device().map(|(d, _)| d);
        self.selected_device = current.as_ref().map(|d| d.key.clone());
        let selection_changed = previous != current;
        if selection_changed {
            self.polling.abandon();
        }
        self.polling
            .retain_inventory(&self.snapshot.devices, &self.configuration_devices);
        if self.dashboard.is_some() && self.page == 1 {
            self.build();
            if selection_changed {
                self.read_polling();
            }
        }
    }
    fn configuration_visibility(&self) {
        self.runtime.send(Command::ConfigurationVisible(
            self.dashboard.is_some() && self.page == 1,
        ));
    }
    fn device_rows(&self) -> Vec<(ConfigurationDevice, Option<DeviceView>)> {
        merged_device_rows(&self.snapshot.devices, &self.configuration_devices)
    }
    fn current_device(&self) -> Option<(ConfigurationDevice, Option<DeviceView>)> {
        current_device_row(
            &self.snapshot.devices,
            &self.configuration_devices,
            self.selected_device.as_deref(),
        )
    }
    fn configuration_name(&self, device: &ConfigurationDevice) -> String {
        self.settings
            .devices
            .get(&device.key)
            .and_then(|p| p.name.clone())
            .unwrap_or_else(|| device.name.clone())
    }
    fn visible_polling_device(&self) -> Option<ConfigurationDevice> {
        (self.dashboard.is_some() && self.page == 1 && self.settings.polling_controls)
            .then(|| self.current_device().map(|(d, _)| d))
            .flatten()
    }
    fn enable_control(&self, id: u16, enabled: bool) {
        if let Some(h) = self.controls.get(&id) {
            unsafe {
                let _ = EnableWindow(*h, enabled);
            }
        }
    }
    fn polling_controls(&mut self) {
        if self.dashboard.is_none() || self.page != 1 {
            return;
        }
        let _redraw = DashboardRedraw::new(self.dashboard.unwrap());
        // Update only this group: an asynchronous hardware reply must not discard unsaved device edits.
        for id in (40..=47).chain([98, 99]) {
            if let Some(h) = self.controls.remove(&id) {
                unsafe {
                    let _ = DestroyWindow(h);
                }
            }
        }
        self.label(98, "Device polling rate", 20, 420, 750);
        self.label(
            99,
            "Hardware configuration only. Apply before starting a game.",
            20,
            448,
            750,
        );
        if !self.settings.polling_controls {
            self.label(
                44,
                &self.current_device().and_then(|(d, _)| match d.capability { PollingCapability::Unavailable(reason) => Some(format!("{reason} Enable polling-rate controls in Settings for supported devices.")), _ => None }).unwrap_or_else(|| "Enable polling-rate controls in Settings to read supported devices.".into()),
                20,
                484,
                750,
            );
            return;
        }
        let Some(reading) = self.visible_polling_device() else {
            self.label(
                44,
                "Select a connected device to read its hardware rate.",
                20,
                484,
                750,
            );
            return;
        };
        let available =
            reading.online() && matches!(reading.capability, PollingCapability::ReadWrite);
        let unavailable = match &reading.capability {
            PollingCapability::Unavailable(reason) => Some(reason.clone()),
            _ if !reading.online() => Some("Device is offline".into()),
            _ => None,
        };
        let key = reading.key;
        let observation = self.polling.observations.get(&key).cloned();
        let current = observation.as_ref().and_then(|o| o.rate);
        self.label(
            44,
            &format!(
                "Device-reported configured rate: {}",
                current.map_or_else(|| "unavailable".into(), |r| format!("{} Hz", r.hz()))
            ),
            20,
            484,
            750,
        );
        let evidence = observation.as_ref().map_or_else(
            || {
                unavailable
                    .clone()
                    .unwrap_or_else(|| "Use Refresh rate to read this device.".into())
            },
            |o| {
                format!(
                    "Last read: {} · {}",
                    polling_timestamp(o.timestamp),
                    o.evidence
                )
            },
        );
        self.control(
            45,
            w!("STATIC"),
            &evidence,
            WINDOW_STYLE::default(),
            20,
            516,
            750,
            48,
        );
        let requested = self
            .settings
            .devices
            .get(&key)
            .and_then(|p| p.requested_polling_rate);
        self.label(
            46,
            &format!(
                "Last requested: {} · Choose a supported rate",
                requested.map_or_else(|| "none".into(), |r| format!("{} Hz", r.hz()))
            ),
            20,
            568,
            750,
        );
        let supported = observation
            .as_ref()
            .map_or_else(Vec::new, |o| o.supported.clone());
        let chosen = requested
            .or(current)
            .and_then(|r| supported.iter().position(|v| *v == r))
            .unwrap_or(0);
        let options = supported
            .iter()
            .map(|r| format!("{} Hz", r.hz()))
            .collect::<Vec<_>>();
        self.combo(43, &options, chosen, 20, 600, 240);
        self.button(40, "&Apply rate", 280, 600, 120);
        self.button(41, "Refresh rate", 410, 600, 140);
        self.button(42, "Restore previous", 560, 600, 190);
        let pending = self.polling.pending.is_some() || !available;
        let verified = observation
            .as_ref()
            .is_some_and(|o| o.target.generation == self.polling.generation && o.rate.is_some());
        self.enable_control(43, !pending && verified && !supported.is_empty());
        self.enable_control(40, !pending && verified && !supported.is_empty());
        self.enable_control(41, !pending);
        let restore = self
            .polling
            .previous
            .get(&key)
            .is_some_and(|r| supported.contains(r) && current != Some(*r));
        self.enable_control(42, !pending && verified && restore);
        let status = self
            .polling
            .status
            .get(&key)
            .cloned()
            .unwrap_or_else(|| "Hardware rate has not been read in this session.".into());
        self.control(
            47,
            w!("STATIC"),
            &status,
            WINDOW_STYLE::default(),
            20,
            638,
            750,
            38,
        );
    }
    fn read_polling(&mut self) {
        if let Some(reading) = self.visible_polling_device()
            && reading.online()
            && matches!(reading.capability, PollingCapability::ReadWrite)
        {
            self.request_polling(reading, PollingIntent::Read);
        }
    }
    fn apply_polling(&mut self, restore: bool) {
        let Some(reading) = self.visible_polling_device() else {
            return;
        };
        let rate = if restore {
            self.polling.previous.get(&reading.key).copied()
        } else {
            self.polling
                .observations
                .get(&reading.key)
                .and_then(|o| o.supported.get(self.choice(43)))
                .copied()
        };
        let Some(rate) = rate else {
            self.polling.status.insert(
                reading.key,
                "Refresh the hardware rate and select a supported value.".into(),
            );
            self.polling_controls();
            return;
        };
        self.request_polling(reading, PollingIntent::Apply { rate, restore });
    }
    fn request_polling(&mut self, reading: ConfigurationDevice, intent: PollingIntent) {
        if self.tray_polling.pending.is_some() || !self.polling_intents.is_empty() {
            self.polling.status.insert(
                reading.key,
                "A hardware configuration request is already pending".into(),
            );
            self.polling_controls();
            return;
        }
        self.polling.sequence = self.polling.sequence.max(self.tray_polling.sequence);
        let key = reading.key.clone();
        match self.polling.begin(reading, intent) {
            Ok(request) => match self.runtime.submit_control(request.clone()) {
                Ok(()) => {
                    if let PollingIntent::Apply { rate, .. } = intent {
                        self.record_polling_intent(&key, request.request, rate);
                    }
                }
                Err(error) => {
                    self.polling.abandon();
                    self.polling.status.insert(key, error.to_string());
                }
            },
            Err(error) => {
                self.polling.status.insert(key, error.into());
            }
        }
        self.polling_controls();
    }
    fn record_polling_intent(&mut self, key: &str, request: u64, rate: PollingRate) {
        self.polling_intents.insert(key.into(), (request, rate));
        // A page close can abandon its UI request while a submitted write still
        // completes. Revoke evidence in both surfaces before the worker replies.
        self.polling.revoke_observation(key);
        self.tray_polling.revoke_observation(key);
        self.tooltip_rates.remove(key);
        self.sync_trays(TrayUpdate::Changed);
    }
    fn polling_outcome(&mut self, outcome: ControlOutcome) {
        cache_tooltip_rate(
            &mut self.tooltip_rates,
            &outcome,
            self.polling.generation.max(self.tray_polling.generation),
            self.settings.polling_controls,
        );
        self.sync_trays(TrayUpdate::Changed);
        if outcome.request & crate::runtime::STARTUP_POLLING_REQUEST_BIT != 0 {
            let initial_read = outcome.request & crate::runtime::STARTUP_POLLING_READ_BIT != 0;
            if let Some(observation) = self.tooltip_rates.get(&outcome.key) {
                self.tray_polling
                    .observations
                    .insert(outcome.key.clone(), observation.clone());
                self.tray_polling.status.insert(
                    outcome.key.clone(),
                    if initial_read {
                        "Startup rate read."
                    } else {
                        "Saved startup rate verified."
                    }
                    .into(),
                );
                if !initial_read && let Some(previous) = outcome.previous {
                    self.tray_polling
                        .previous
                        .insert(outcome.key.clone(), previous);
                }
            } else if let Some(failure) = &outcome.failure {
                self.tray_polling.status.insert(
                    outcome.key.clone(),
                    format!(
                        "Startup {}: {failure}",
                        if initial_read { "rate read" } else { "restore" }
                    ),
                );
                if !initial_read {
                    self.tray_polling_feedback(&outcome.key);
                }
            }
            return;
        }
        // Preserve explicit intent even when the user has left the page; never use an observation as a setting.
        if let Some(&(request, rate)) = self.polling_intents.get(&outcome.key)
            && request == outcome.request
        {
            self.polling_intents.remove(&outcome.key);
            self.polling.revoke_observation(&outcome.key);
            self.tray_polling.revoke_observation(&outcome.key);
            self.settings
                .devices
                .entry(outcome.key.clone())
                .or_default()
                .requested_polling_rate = Some(rate);
            self.save();
        }
        let tray_key = self
            .tray_polling
            .pending
            .as_ref()
            .filter(|p| p.request == outcome.request && p.key == outcome.key)
            .map(|p| (p.key.clone(), p.intent));
        if let Some((key, intent)) = tray_key {
            if self.tray_polling.accept(&outcome, Some(&key)) {
                if intent == PollingIntent::Read
                    && outcome.failure.is_none()
                    && self
                        .tray_polling
                        .observations
                        .get(&key)
                        .is_some_and(|o| o.rate.is_some())
                {
                    self.tray_polling.status.insert(
                        key.clone(),
                        "Hardware rate read. Select a rate in the polling menu to apply.".into(),
                    );
                }
                // A tray operation invalidates the dashboard's cached read so it
                // cannot present the previous rate as the newly confirmed value.
                self.polling.observations.remove(&key);
                self.tray_polling_feedback(&key);
                self.polling_controls();
            }
            return;
        }
        let key = self.visible_polling_device().map(|r| r.key);
        if self.polling.accept(&outcome, key.as_deref()) {
            self.tray_polling.observations.remove(&outcome.key);
            self.polling_controls();
        }
    }
    fn release_history(&mut self) {
        self.request = self.request.wrapping_add(1);
        self.series = HistorySeries {
            axis: self.history.axis,
            ..Default::default()
        };
    }
    fn history_outcome(&mut self, id: u64, result: Result<HistorySeries, ProviderError>) {
        if self.dashboard.is_none() || self.page != 2 || id != self.request {
            return;
        }
        match result {
            Ok(series) => self.series = series,
            Err(e) => self.error = e.to_string(),
        }
        unsafe {
            if let Some(h) = self.dashboard {
                let _ = InvalidateRect(Some(h), None, false);
            }
        }
    }
    fn query(&mut self) {
        self.request = self.request.wrapping_add(1);
        self.series = HistorySeries {
            axis: self.history.axis,
            ..Default::default()
        };
        let Some(d) = self.snapshot.devices.get(self.selected) else {
            return;
        };
        let until = SystemClock::default().unix();
        let mut r = RECT::default();
        unsafe {
            if let Some(h) = self.dashboard {
                let _ = GetClientRect(h, &mut r);
                let _ = InvalidateRect(Some(h), None, false);
            }
        }
        self.runtime.send(self.history.command(
            d.reading.key.clone(),
            until,
            (r.right - 80).max(20) as usize,
            self.request,
        ));
    }
    fn drain(&mut self) {
        let mut latest = None;
        let mut polling_outcomes = Vec::new();
        let mut polling_invalidated = false;
        let mut polling_selection_changed = false;
        while let Ok(e) = self.runtime.events.try_recv() {
            match e {
                Event::Snapshot(s) => latest = Some(s),
                Event::ConfigurationInventory {
                    generation,
                    devices,
                    failure,
                } => {
                    self.configuration_inventory(generation, devices, failure);
                }
                Event::Polling(outcome) => polling_outcomes.push(*outcome),
                Event::PollingInvalidated(generation) => {
                    // Epoch changes revoke actionable evidence, not the labelled
                    // last-confirmed tooltip for a still-matching physical device.
                    // Snapshots remove missing/stale/replaced identities separately.
                    self.polling.invalidate(generation);
                    self.tray_polling.invalidate(generation);
                    polling_invalidated = true;
                }
                Event::Diagnostics(d) => self.diagnostics = d,
                Event::Error(e) => self.error = e,
                Event::Alert(n) => {
                    let result = self
                        .trays
                        .get_mut(&n.key)
                        .ok_or_else(|| ProviderError::new("Tray icon not available"))
                        .and_then(|t| t.deliver(&n));
                    if result.is_err() {
                        self.runtime.send(Command::NotificationFailed(n));
                    }
                }
                Event::Insights(id, result) if self.dashboard.is_some() && self.page == 6 => {
                    let key = self
                        .snapshot
                        .devices
                        .get(self.selected)
                        .map(|d| d.reading.key.as_str());
                    if self.insights.accept(id, key, result) {
                        self.render_insights();
                    }
                }
                Event::History(id, result) => self.history_outcome(id, result),
                _ => {}
            }
        }
        if let Some(s) = latest {
            let changed_labels: Vec<_> = s
                .devices
                .iter()
                .enumerate()
                .filter_map(|(index, next)| {
                    (self
                        .snapshot
                        .devices
                        .get(index)
                        .map(|previous| &previous.text)
                        != Some(&next.text))
                    .then_some(index)
                })
                .collect();
            let identity: Vec<_> = self
                .snapshot
                .devices
                .iter()
                .map(|d| d.reading.key.clone())
                .collect();
            let next: Vec<_> = s.devices.iter().map(|d| d.reading.key.clone()).collect();
            let previous_key = self
                .snapshot
                .devices
                .get(self.selected)
                .map(|d| d.reading.key.clone());
            let previous_device = self.current_device().map(|(d, _)| d);
            self.snapshot = s;
            self.tooltip_rates.retain(|key, observation| {
                self.snapshot.devices.iter().any(|d| {
                    &d.reading.key == key
                        && d.reading.connection != Connection::Stale
                        && observation.target.device.matches_reading(&d.reading)
                })
            });
            self.tray_polling
                .retain_inventory(&self.snapshot.devices, &[]);
            let current_device = self.current_device().map(|(d, _)| d);
            self.selected_device = current_device.as_ref().map(|d| d.key.clone());
            if previous_device != current_device {
                self.polling.abandon();
                polling_selection_changed = true;
            }
            self.polling
                .retain_inventory(&self.snapshot.devices, &self.configuration_devices);
            self.selected = previous_key
                .as_ref()
                .and_then(|key| {
                    self.snapshot
                        .devices
                        .iter()
                        .position(|d| &d.reading.key == key)
                })
                .unwrap_or_else(|| {
                    self.selected
                        .min(self.snapshot.devices.len().saturating_sub(1))
                });
            let selected_key = self
                .snapshot
                .devices
                .get(self.selected)
                .map(|d| &d.reading.key);
            if previous_key.as_ref() != selected_key {
                self.polling.abandon();
                self.insights.select(selected_key.cloned());
                polling_selection_changed = true;
            }
            if self.page == 1
                && let Some((_, Some(d))) = self.current_device()
                && self.controls.contains_key(&91)
            {
                self.set_control_text(91, &device_detail(&d));
            }
            self.sync_trays(TrayUpdate::Changed);
            if identity != next {
                self.build()
            } else if let Some(h) = self.controls.get(&10) {
                for i in changed_labels {
                    let d = &self.snapshot.devices[i];
                    let text = wide(&d.text);
                    unsafe {
                        send(*h, CB_DELETESTRING, WPARAM(i), LPARAM(0));
                        send(
                            *h,
                            CB_INSERTSTRING,
                            WPARAM(i),
                            LPARAM(text.as_ptr() as isize),
                        );
                    }
                }
                unsafe {
                    let selection = if self.page == 1 {
                        self.selected_device
                            .as_ref()
                            .and_then(|key| {
                                device_row_index(
                                    &self.snapshot.devices,
                                    &self.configuration_devices,
                                    key,
                                )
                            })
                            .unwrap_or(0)
                    } else {
                        self.selected
                    };
                    if send(*h, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 != selection as isize {
                        send(*h, CB_SETCURSEL, WPARAM(selection), LPARAM(0));
                    }
                }
            }
        }
        // Startup readback can arrive before its first battery snapshot. Apply
        // the inventory first; generation checks still reject invalidated replies.
        for outcome in polling_outcomes {
            self.polling_outcome(outcome);
        }
        if polling_invalidated || polling_selection_changed {
            self.polling_controls();
            self.read_polling();
        }
    }
    fn sync_trays(&mut self, update: TrayUpdate) {
        let dark = self.tray_theme.read(
            Instant::now(),
            &self.settings.icon_theme,
            update != TrayUpdate::Changed,
            || tray_dark(&self.settings),
        );
        let settings = &self.settings;
        let devices = tray_devices(&self.snapshot, settings);
        self.trays.retain(|key, _| {
            devices.iter().any(|d| {
                &d.reading.key == key
                    && !settings.devices.get(key).is_some_and(|p| p.hidden)
                    && !d.hidden
            })
        });
        for d in devices.iter().filter(|d| {
            !d.hidden
                && !settings
                    .devices
                    .get(&d.reading.key)
                    .is_some_and(|p| p.hidden)
        }) {
            let existing = self.trays.get_mut(&d.reading.key);
            if let Some(t) = existing {
                let mut changed = false;
                if !t.signature.matches(d, settings, dark)
                    && let Ok(frames) = icons::frames(d, settings, dark, 32)
                {
                    t.frames = frames;
                    t.signature = icon_signature(d, settings, dark);
                    t.frame = 0;
                    changed = true
                }
                let old = t.data.szTip;
                copy_tray_tooltip(
                    &mut t.data.szTip,
                    d,
                    self.tooltip_rates
                        .get(&d.reading.key)
                        .filter(|_| settings.polling_controls),
                );
                changed |= old != t.data.szTip;
                t.data.hIcon = t.frames[t.frame].0;
                t.registration
                    .update_with(&t.data, update, changed, &mut shell_notify);
            } else if let Ok(frames) = icons::frames(d, settings, dark, 32) {
                let mut data = NOTIFYICONDATAW {
                    cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                    hWnd: self.monitor,
                    uID: stable_guid(&d.reading.key) as u32,
                    uFlags: NIF_GUID | NIF_ICON | NIF_MESSAGE | NIF_TIP,
                    uCallbackMessage: TRAY,
                    hIcon: frames[0].0,
                    guidItem: GUID::from_u128(stable_guid(&d.reading.key)),
                    ..Default::default()
                };
                copy_tray_tooltip(
                    &mut data.szTip,
                    d,
                    self.tooltip_rates
                        .get(&d.reading.key)
                        .filter(|_| settings.polling_controls),
                );
                let mut registration = TrayRegistration::default();
                registration.update_with(&data, update, true, &mut shell_notify);
                self.trays.insert(
                    d.reading.key.clone(),
                    Tray {
                        registration,
                        data,
                        frames,
                        signature: icon_signature(d, settings, dark),
                        frame: 0,
                    },
                );
            }
        }
        let animate = self.trays.values().any(|t| t.frames.len() > 1);
        if animate != self.animating {
            unsafe {
                if animate {
                    SetTimer(Some(self.monitor), 2, 100, None);
                } else {
                    let _ = KillTimer(Some(self.monitor), 2);
                }
            }
            self.animating = animate;
        }
    }
    fn animate(&mut self) {
        for t in self.trays.values_mut().filter(|t| t.frames.len() > 1) {
            t.frame = (t.frame + 1) % t.frames.len();
            let mut data = t.data;
            data.uFlags = NIF_GUID | NIF_ICON;
            data.hIcon = t.frames[t.frame].0;
            unsafe {
                let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
            }
        }
    }
    fn menu(&mut self, key: Option<String>) {
        unsafe {
            let Ok(menu) = popup_menu(&self.snapshot, &self.settings, key.as_deref()) else {
                return;
            };
            // Build from cached metadata only; opening/hovering never performs HID I/O.
            let choices = key
                .as_deref()
                .and_then(|key| self.tray_polling_device(key))
                .map(|device| {
                    tray_polling_choices(
                        &device,
                        &self.settings,
                        &self.tray_polling,
                        self.polling.pending.is_some() || !self.polling_intents.is_empty(),
                    )
                })
                .unwrap_or_default();
            if !choices.is_empty() && append_polling_menu(menu.0, &choices).is_err() {
                return;
            }
            let palette = Palette::new(
                hb_windows::system::dashboard_dark_theme(),
                hb_windows::system::high_contrast(),
            );
            let appearance = PopupAppearance::new(menu.0, self.monitor, palette);
            (*self.context).popup.replace(appearance);
            let mut point = POINT::default();
            let _ = GetCursorPos(&mut point);
            let _ = SetForegroundWindow(self.monitor);
            let chosen = TrackPopupMenu(
                menu.0,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                Some(0),
                self.monitor,
                None,
            );
            // Destroy the native menu before releasing the brush it references.
            drop(menu);
            (*self.context).popup.take();
            if let Some(choice) = choices
                .iter()
                .find(|choice| choice.id == chosen.0 as u16 && choice.enabled && choice.id != 0)
            {
                if let Some(key) = key.as_deref() {
                    self.tray_polling_command(key, choice.action);
                }
            } else if chosen.0 != 0 {
                self.command(chosen.0 as u16, 0)
            }
            let _ = PostMessageW(Some(self.monitor), WM_APP + 7, WPARAM(0), LPARAM(0));
        }
    }
    fn tray_polling_device(&self, key: &str) -> Option<ConfigurationDevice> {
        let reading = &self
            .snapshot
            .devices
            .iter()
            .find(|d| d.reading.key == key && !d.hidden)?
            .reading;
        let device = ConfigurationDevice::from_reading(reading);
        hb_providers::controls::polling_menu_candidate(&device).then_some(device)
    }
    fn tray_polling_command(&mut self, key: &str, action: TrayPollingAction) {
        if action == TrayPollingAction::Settings {
            self.page = 3;
            self.configuration_visibility();
            self.open();
            self.build();
            return;
        }
        let Some(device) = self.tray_polling_device(key) else {
            return;
        };
        if !self.settings.polling_controls
            || self.polling.pending.is_some()
            || !self.polling_intents.is_empty()
        {
            return;
        }
        let intent = match action {
            TrayPollingAction::Read => PollingIntent::Read,
            TrayPollingAction::Apply(rate) => PollingIntent::Apply {
                rate,
                restore: false,
            },
            TrayPollingAction::Restore => {
                let Some(&rate) = self.tray_polling.previous.get(key) else {
                    return;
                };
                PollingIntent::Apply {
                    rate,
                    restore: true,
                }
            }
            TrayPollingAction::Settings | TrayPollingAction::None => return,
        };
        self.tray_polling.sequence = self.tray_polling.sequence.max(self.polling.sequence);
        match self.tray_polling.begin_tray(device, intent) {
            Ok(request) => match self.runtime.submit_control(request.clone()) {
                Ok(()) => {
                    if let PollingIntent::Apply { rate, .. } = intent {
                        self.record_polling_intent(key, request.request, rate);
                    }
                }
                Err(error) => {
                    self.tray_polling.abandon();
                    self.tray_polling
                        .status
                        .insert(key.into(), error.to_string());
                    self.tray_polling_feedback(key);
                }
            },
            Err(error) => {
                self.tray_polling.status.insert(key.into(), error.into());
                self.tray_polling_feedback(key);
            }
        }
    }
    fn tray_polling_feedback(&mut self, key: &str) {
        if let Some(message) = self.tray_polling.status.get(key)
            && let Some(tray) = self.trays.get_mut(key)
        {
            let _ = tray.show_message("Polling rate", message);
        }
    }
}

unsafe fn send(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { SendMessageW(h, m, Some(w), Some(l)) }
}

#[derive(Debug, PartialEq, Eq)]
struct IconSignature {
    level: Option<u8>,
    precision: Precision,
    charging: Option<bool>,
    connection: Connection,
    icon: String,
    low_alert_at: u8,
    dark: bool,
    icon_theme: String,
    animation: bool,
    percent_in_icon: bool,
    badges: bool,
    warning_level: u8,
}
impl IconSignature {
    fn matches(&self, d: &DeviceView, settings: &Settings, dark: bool) -> bool {
        self.level == d.reading.level
            && self.precision == d.reading.precision
            && self.charging == d.reading.charging
            && self.connection == d.reading.connection
            && self.icon == d.icon
            && self.low_alert_at == d.low_alert_at
            && self.dark == dark
            && self.icon_theme == settings.icon_theme
            && self.animation == settings.animation
            && self.percent_in_icon == settings.percent_in_icon
            && self.badges == settings.badges
            && self.warning_level == settings.warning_level
    }
}
fn icon_signature(d: &DeviceView, settings: &Settings, dark: bool) -> IconSignature {
    IconSignature {
        level: d.reading.level,
        precision: d.reading.precision.clone(),
        charging: d.reading.charging,
        connection: d.reading.connection.clone(),
        icon: d.icon.clone(),
        low_alert_at: d.low_alert_at,
        dark,
        icon_theme: settings.icon_theme.clone(),
        animation: settings.animation,
        percent_in_icon: settings.percent_in_icon,
        badges: settings.badges,
        warning_level: settings.warning_level,
    }
}

fn tray_dark(settings: &Settings) -> bool {
    let fallback = hb_windows::system::dark_theme();
    if !["auto", "topbar"].contains(&settings.icon_theme.as_str())
        || !hb_windows::system::mydockfinder_running()
    {
        return fallback;
    }
    unsafe {
        let dc = GetDC(None);
        if dc.0.is_null() {
            return fallback;
        }
        let width = GetSystemMetrics(SM_CXSCREEN);
        let mut luminance = 0.;
        let mut count = 0;
        for i in 1..20 {
            let c = GetPixel(dc, width * i / 20, 8).0;
            if c != 0xffffffff {
                luminance += 0.2126 * (c & 255) as f64
                    + 0.7152 * ((c >> 8) & 255) as f64
                    + 0.0722 * ((c >> 16) & 255) as f64;
                count += 1
            }
        }
        ReleaseDC(None, dc);
        if count == 0 {
            fallback
        } else {
            luminance / (count as f64) < 123.
        }
    }
}

impl NotificationSink for Tray {
    fn deliver(&mut self, n: &Notification) -> Result<(), ProviderError> {
        self.show_message(&n.title, &n.text)
    }
}
impl Tray {
    fn show_message(&mut self, title: &str, text: &str) -> Result<(), ProviderError> {
        let mut data = self.data;
        data.uFlags = NIF_GUID | NIF_INFO;
        copy(&mut data.szInfoTitle, title);
        copy(&mut data.szInfo, text);
        data.dwInfoFlags = NIIF_INFO;
        if unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool() {
            Ok(())
        } else {
            Err(ProviderError::new(
                "Windows could not deliver the notification",
            ))
        }
    }
}

#[cfg(test)]
pub(crate) static NATIVE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn device_detail(d: &DeviceView) -> String {
    let age = (SystemClock::default().unix() - d.reading.timestamp).max(0);
    let fresh = if age < 60 {
        format!("{age}s ago")
    } else {
        format!("{}m ago", age / 60)
    };
    let charging = match d.reading.charging {
        Some(true) => {
            if d.reading.charging_inferred {
                "charging (inferred)"
            } else {
                "charging"
            }
        }
        Some(false) => "not charging",
        None => "charging unknown",
    };
    format!(
        "{} · {:?} · {} · read {}",
        provider_label(&d.reading.source),
        d.reading.connection,
        charging,
        fresh
    )
}
fn provider_label(id: &str) -> &str {
    match id {
        "razer" => "Razer",
        "audeze" => "Audeze Maxwell",
        "wlmouse" => "WLmouse",
        "mchose" => "MCHOSE",
        "hyperx_alpha2" => "HyperX Alpha 2",
        "hyperx_cloud3" => "HyperX Cloud III",
        "hyperx" => "HyperX Cloud II",
        "keychron" => "Keychron",
        "pulsar" => "Pulsar / ATK / VXE",
        "jbl" => "JBL Quantum",
        "logitech" => "Logitech",
        "steelseries" => "SteelSeries",
        "playstation" => "PlayStation",
        "eightbitdo" => "8BitDo",
        "barracuda" => "Barracuda Pro",
        "nintendo" => "Nintendo Switch",
        "asus" => "ASUS ROG / TUF",
        "gwolves" => "G-Wolves",
        "lofree" => "Lofree",
        "astro" => "Astro A50",
        "corsair" => "Corsair",
        "lamzu" => "LAMZU",
        "am_infinity" => "AM Infinity 8K",
        "bluetooth" => "Bluetooth",
        "xinput" => "Xbox controllers",
        other => other,
    }
}
fn device_threshold(text: &str) -> Result<Option<u8>, &'static str> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    text.trim()
        .parse::<u8>()
        .ok()
        .filter(|v| *v <= 100)
        .map(Some)
        .ok_or("Enter 0–100, or leave blank for the default alert threshold")
}

fn tray_devices<'a>(snapshot: &'a Snapshot, settings: &Settings) -> Vec<Cow<'a, DeviceView>> {
    let mut devices: Vec<_> = snapshot
        .devices
        .iter()
        .filter(|d| {
            !d.hidden
                && !settings
                    .devices
                    .get(&d.reading.key)
                    .is_some_and(|p| p.hidden)
        })
        .map(Cow::Borrowed)
        .collect();
    if devices.is_empty() {
        devices.push(Cow::Owned(DeviceView {
            reading: Reading::new("application", "Halo Battery Next", "app", 0),
            name: "Halo Battery Next".into(),
            icon: "mouse".into(),
            low_alert_at: 0,
            seconds_left: None,
            text: "Halo Battery Next — no visible devices".into(),
            hidden: false,
        }));
    }
    devices
}

#[cfg(test)]
mod behaviour_tests {
    use super::*;
    #[test]
    fn icon_changes_on_precision_transition_but_not_timestamp_refresh() {
        let settings = Settings::default();
        let mut d = tray_devices(&Snapshot::default(), &settings)
            .remove(0)
            .into_owned();
        d.reading.level = Some(50);
        let exact = icon_signature(&d, &settings, false);
        d.reading.timestamp += 60;
        assert_eq!(exact, icon_signature(&d, &settings, false));
        d.reading.precision = Precision::Coarse;
        assert_ne!(exact, icon_signature(&d, &settings, false));
    }
    #[test]
    fn tray_cache_signature_covers_visual_changes_but_ignores_text_and_freshness() {
        let settings = Settings::default();
        let device = tray_devices(&Snapshot::default(), &settings)
            .remove(0)
            .into_owned();
        let signature = icon_signature(&device, &settings, false);
        type Edit = fn(&mut DeviceView, &mut Settings);
        let edits: [Edit; 11] = [
            |d, _| d.reading.level = Some(50),
            |d, _| d.reading.precision = Precision::Coarse,
            |d, _| d.reading.charging = Some(true),
            |d, _| d.reading.connection = Connection::Sleeping,
            |d, _| d.icon = "keyboard".into(),
            |d, _| d.low_alert_at += 1,
            |_, s| s.icon_theme = "black".into(),
            |_, s| s.animation = !s.animation,
            |_, s| s.percent_in_icon = !s.percent_in_icon,
            |_, s| s.badges = !s.badges,
            |_, s| s.warning_level += 1,
        ];
        for edit in edits {
            let mut d = device.clone();
            let mut s = settings.clone();
            edit(&mut d, &mut s);
            assert!(!signature.matches(&d, &s, false));
        }
        assert!(!signature.matches(&device, &settings, true));
        let mut text_only = device;
        text_only.reading.timestamp += 60;
        text_only.reading.charging_inferred = true;
        text_only.name = "Renamed".into();
        text_only.text = "Fresh tooltip".into();
        assert!(signature.matches(&text_only, &settings, false));
    }
    #[test]
    fn fixed_utf16_buffers_remain_terminated_and_clear_previous_content() {
        let mut buffer = [99; 6];
        copy(&mut buffer, "A😀BCDE");
        assert_eq!(buffer, [65, 0xd83d, 0xde00, 66, 67, 0]);
        copy(&mut buffer, "X");
        assert_eq!(buffer, [88, 0, 0, 0, 0, 0]);
        copy(&mut [], "anything");
        let mut one = [99];
        copy(&mut one, "anything");
        assert_eq!(one, [0]);
    }
    #[test]
    fn placeholder_exists_only_without_visible_devices() {
        let mut settings = Settings::default();
        let mut snapshot = Snapshot::default();
        let empty = tray_devices(&snapshot, &settings)
            .into_iter()
            .map(Cow::into_owned)
            .collect::<Vec<_>>();
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0].reading.key, "application");
        let mut reading = Reading::new("mouse", "Mouse", "razer", SystemClock::default().unix());
        reading.level = Some(50);
        snapshot.devices.push(DeviceView {
            reading,
            name: "Mouse".into(),
            icon: "mouse".into(),
            low_alert_at: 20,
            seconds_left: None,
            text: "Mouse: 50%".into(),
            hidden: false,
        });
        let visible = tray_devices(&snapshot, &settings);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].reading.key, "mouse");
        settings.devices.entry("mouse".into()).or_default().hidden = true;
        let hidden = tray_devices(&snapshot, &settings);
        assert_eq!(hidden.len(), 1);
        assert_eq!(hidden[0].reading.key, "application");
        assert_eq!(
            stable_guid(&empty[0].reading.key),
            stable_guid(&hidden[0].reading.key)
        );
        settings.devices.clear();
        snapshot.devices[0].hidden = true;
        assert_eq!(
            tray_devices(&snapshot, &settings)[0].reading.key,
            "application"
        );
    }
    #[test]
    fn every_provider_has_a_unique_readable_settings_label() {
        let ids = providers();
        assert_eq!(ids.len(), 25);
        let labels = ids
            .iter()
            .map(|id| provider_label(id))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(labels.len(), 25);
        assert_eq!(provider_label("razer"), "Razer");
        assert_eq!(provider_label("playstation"), "PlayStation");
        assert!(ids.iter().all(|id| !provider_label(id).contains('_')));
    }
    #[test]
    fn clearing_device_threshold_restores_default_without_losing_other_preferences() {
        assert_eq!(device_threshold(" "), Ok(None));
        assert_eq!(device_threshold("30"), Ok(Some(30)));
        assert_eq!(device_threshold("0"), Ok(Some(0)));
        assert!(device_threshold("101").is_err());
        assert!(device_threshold("-1").is_err());
        assert!(device_threshold("bad").is_err());
        let mut settings = Settings::default();
        let mut preference = DevicePreferences {
            name: Some("Custom mouse".into()),
            low: device_threshold("30").unwrap(),
            ..Default::default()
        };
        settings.devices.insert("mouse".into(), preference.clone());
        assert_eq!(settings.low_for("mouse"), 30);
        preference.low = device_threshold("").unwrap();
        settings.devices.insert("mouse".into(), preference);
        settings.low = 15;
        assert_eq!(settings.low_for("mouse"), 15);
        assert_eq!(
            settings.devices["mouse"].name.as_deref(),
            Some("Custom mouse")
        );
    }
    #[test]
    fn device_details_show_freshness_and_charging_uncertainty() {
        let mut reading = Reading::new(
            "mouse",
            "Mouse",
            "razer",
            SystemClock::default().unix() - 120,
        );
        reading.charging = Some(false);
        reading.connection = Connection::Stale;
        let mut d = DeviceView {
            reading,
            name: "Mouse".into(),
            icon: "mouse".into(),
            low_alert_at: 20,
            seconds_left: None,
            text: "Mouse".into(),
            hidden: false,
        };
        let text = device_detail(&d);
        assert!(text.contains("Stale"));
        assert!(text.contains("not charging"));
        assert!(text.contains("2m ago"));
        d.reading.charging = None;
        assert!(device_detail(&d).contains("charging unknown"));
        d.reading.charging = Some(true);
        d.reading.charging_inferred = true;
        assert!(device_detail(&d).contains("charging (inferred)"));
    }
}

/// Documented owner drawing keeps native popup navigation, command IDs, item
/// strings and accessibility. High contrast retains the system's native menu.
struct PopupItem {
    menu: HMENU,
    position: u32,
    submenu: bool,
    native_type: MENU_ITEM_TYPE,
    native_data: usize,
    text: Vec<u16>,
    separator: bool,
    enabled: bool,
}
struct PopupAppearance {
    menu: HMENU,
    menus: Vec<(HMENU, HBRUSH)>,
    palette: Palette,
    items: Vec<PopupItem>,
    font: HFONT,
    background: HBRUSH,
    width: u32,
    row_height: u32,
    padding: i32,
}
impl Drop for PopupAppearance {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.font.into());
            let _ = DeleteObject(self.background.into());
        }
    }
}
fn menu_mnemonic(text: &[u16]) -> Option<char> {
    let mut characters = String::from_utf16_lossy(text)
        .chars()
        .collect::<Vec<_>>()
        .into_iter();
    while let Some(character) = characters.next() {
        if character == '&' {
            let next = characters.next()?;
            if next != '&' {
                return Some(next.to_ascii_lowercase());
            }
        }
    }
    None
}
impl PopupAppearance {
    fn new(menu: HMENU, owner: HWND, palette: Palette) -> Option<Rc<Self>> {
        if palette.high_contrast {
            return None;
        }
        unsafe {
            let dpi = GetDpiForWindow(owner).max(96);
            let padding = (18 * dpi / 96) as i32;
            let font = CreateFontW(
                -((16 * dpi / 96) as i32),
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                DEFAULT_PITCH.0 as u32,
                w!("Segoe UI"),
            );
            let mut appearance = Self {
                menu,
                menus: Vec::new(),
                palette,
                items: Vec::new(),
                font,
                background: CreateSolidBrush(palette.surface),
                width: 0,
                row_height: 30 * dpi / 96,
                padding,
            };
            if appearance.font.is_invalid() || appearance.background.is_invalid() {
                return None;
            }
            let hdc = GetDC(Some(owner));
            if hdc.is_invalid() {
                return None;
            }
            let saved = SaveDC(hdc);
            if saved == 0 {
                ReleaseDC(Some(owner), hdc);
                return None;
            }
            SelectObject(hdc, font.into());
            let mut pending_menus = vec![menu];
            let mut next_menu = 0;
            while next_menu < pending_menus.len() {
                let menu = pending_menus[next_menu];
                next_menu += 1;
                let mut native_menu = MENUINFO {
                    cbSize: size_of::<MENUINFO>() as u32,
                    fMask: MIM_BACKGROUND,
                    ..Default::default()
                };
                if GetMenuInfo(menu, &mut native_menu).is_err() {
                    let _ = RestoreDC(hdc, saved);
                    ReleaseDC(Some(owner), hdc);
                    return None;
                }
                appearance.menus.push((menu, native_menu.hbrBack));
                for index in 0..GetMenuItemCount(Some(menu)).max(0) as u32 {
                    let mut info = MENUITEMINFOW {
                        cbSize: size_of::<MENUITEMINFOW>() as u32,
                        fMask: MIIM_FTYPE | MIIM_STATE | MIIM_SUBMENU | MIIM_DATA,
                        ..Default::default()
                    };
                    if GetMenuItemInfoW(menu, index, true, &mut info).is_err() {
                        let _ = RestoreDC(hdc, saved);
                        ReleaseDC(Some(owner), hdc);
                        return None;
                    }
                    let mut text = [0u16; 2048];
                    let length =
                        GetMenuStringW(menu, index, Some(&mut text), MF_BYPOSITION).max(0) as usize;
                    let text = text[..length].to_vec();
                    let mut rect = RECT::default();
                    let mut measure = text.clone();
                    if !measure.is_empty() {
                        DrawTextW(hdc, &mut measure, &mut rect, DT_SINGLELINE | DT_CALCRECT);
                    }
                    appearance.width = appearance
                        .width
                        .max((rect.right - rect.left).max(0) as u32 + (padding * 2) as u32);
                    appearance.items.push(PopupItem {
                        menu,
                        position: index,
                        submenu: !info.hSubMenu.is_invalid(),
                        native_type: info.fType,
                        native_data: info.dwItemData,
                        text,
                        separator: info.fType.0 & MFT_SEPARATOR.0 != 0,
                        enabled: info.fState.0 & (MFS_DISABLED.0 | MFS_GRAYED.0) == 0,
                    });
                    if !info.hSubMenu.is_invalid() && !pending_menus.contains(&info.hSubMenu) {
                        pending_menus.push(info.hSubMenu);
                    }
                }
            }
            let _ = RestoreDC(hdc, saved);
            ReleaseDC(Some(owner), hdc);
            // Strings remain MIIM_STRING data for accessibility and inspection.
            for (index, item) in appearance.items.iter().enumerate() {
                let info = MENUITEMINFOW {
                    cbSize: size_of::<MENUITEMINFOW>() as u32,
                    fMask: MIIM_FTYPE | MIIM_DATA,
                    fType: MFT_OWNERDRAW
                        | if item.separator {
                            MFT_SEPARATOR
                        } else {
                            MFT_STRING
                        },
                    dwItemData: index + 1,
                    ..Default::default()
                };
                if SetMenuItemInfoW(item.menu, item.position, true, &info).is_err() {
                    appearance.restore_native();
                    return None;
                }
            }
            let info = MENUINFO {
                cbSize: size_of::<MENUINFO>() as u32,
                fMask: MIM_BACKGROUND,
                hbrBack: appearance.background,
                ..Default::default()
            };
            for (menu, _) in &appearance.menus {
                if SetMenuInfo(*menu, &info).is_err() {
                    appearance.restore_native();
                    return None;
                }
            }
            Some(Rc::new(appearance))
        }
    }
    fn restore_native(&self) {
        unsafe {
            for item in &self.items {
                let info = MENUITEMINFOW {
                    cbSize: size_of::<MENUITEMINFOW>() as u32,
                    fMask: MIIM_FTYPE | MIIM_DATA,
                    fType: item.native_type,
                    dwItemData: item.native_data,
                    ..Default::default()
                };
                let _ = SetMenuItemInfoW(item.menu, item.position, true, &info);
            }
            // Original brushes belong to the menus' caller; never delete them.
            for (menu, background) in &self.menus {
                let info = MENUINFO {
                    cbSize: size_of::<MENUINFO>() as u32,
                    fMask: MIM_BACKGROUND,
                    hbrBack: *background,
                    ..Default::default()
                };
                let _ = SetMenuInfo(*menu, &info);
            }
        }
    }
    fn mnemonic(&self, character: char) -> LRESULT {
        self.mnemonic_in(self.menu, character)
    }
    fn mnemonic_in(&self, menu: HMENU, character: char) -> LRESULT {
        let matches = self
            .items
            .iter()
            .filter(|item| {
                item.menu == menu
                    && item.enabled
                    && menu_mnemonic(&item.text) == Some(character.to_ascii_lowercase())
            })
            .map(|item| item.position)
            .collect::<Vec<_>>();
        if matches.is_empty() {
            return LRESULT((MNC_IGNORE << 16) as isize);
        }
        let selected = matches.iter().position(|index| unsafe {
            GetMenuState(menu, *index, MF_BYPOSITION) & MF_HILITE.0 != 0
        });
        let index = matches[selected.map_or(0, |selected| (selected + 1) % matches.len())];
        let action = if matches.len() == 1 {
            MNC_EXECUTE
        } else {
            MNC_SELECT
        };
        LRESULT((index | action << 16) as isize)
    }
    fn message(&self, message: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if !matches!(message, WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR) {
            return None;
        }
        unsafe {
            if message == WM_MENUCHAR {
                let menu = HMENU(lp.0 as *mut _);
                if !self.menus.iter().any(|(registered, _)| *registered == menu) {
                    return None;
                }
                let character = char::from_u32((wp.0 & 0xffff) as u32)?;
                return Some(if menu == self.menu {
                    self.mnemonic(character)
                } else {
                    self.mnemonic_in(menu, character)
                });
            }
            if lp.0 == 0 {
                return None;
            }
            if message == WM_MEASUREITEM {
                let item = &mut *(lp.0 as *mut MEASUREITEMSTRUCT);
                if item.CtlType != ODT_MENU {
                    return None;
                }
                let entry = self.items.get(item.itemData.checked_sub(1)?)?;
                item.itemWidth = self.width;
                item.itemHeight = if entry.separator {
                    (self.row_height / 3).max(1)
                } else {
                    self.row_height
                };
                return Some(LRESULT(1));
            }
            let item = &*(lp.0 as *const DRAWITEMSTRUCT);
            if item.CtlType != ODT_MENU {
                return None;
            }
            let entry = self.items.get(item.itemData.checked_sub(1)?)?;
            if item.hwndItem.0 != entry.menu.0 {
                return None;
            }
            let saved = SaveDC(item.hDC);
            SelectObject(item.hDC, self.font.into());
            let selected = item.itemState.0 & ODS_SELECTED.0 != 0;
            let disabled = item.itemState.0 & (ODS_DISABLED.0 | ODS_GRAYED.0) != 0;
            FillRect(
                item.hDC,
                &item.rcItem,
                color_brush(
                    item.hDC,
                    if selected && !disabled {
                        self.palette.selection
                    } else {
                        self.palette.surface
                    },
                ),
            );
            let mut rect = item.rcItem;
            rect.left += self.padding;
            rect.right -= self.padding;
            if entry.separator {
                rect.top = (rect.top + rect.bottom) / 2;
                rect.bottom = rect.top + 1;
                FillRect(item.hDC, &rect, color_brush(item.hDC, self.palette.border));
            } else {
                SetBkMode(item.hDC, TRANSPARENT);
                SetTextColor(
                    item.hDC,
                    if disabled {
                        self.palette.disabled
                    } else if selected {
                        self.palette.selection_text
                    } else {
                        self.palette.text
                    },
                );
                let mut text = entry.text.clone();
                let mut flags = DT_SINGLELINE | DT_VCENTER;
                if item.itemState.0 & ODS_NOACCEL.0 != 0 {
                    flags |= DT_HIDEPREFIX;
                }
                if !text.is_empty() {
                    DrawTextW(item.hDC, &mut text, &mut rect, flags);
                }
                if item.itemState.0 & ODS_CHECKED.0 != 0 {
                    let mut check_rect = item.rcItem;
                    check_rect.right = check_rect.left + self.padding;
                    let mut check = ['✓' as u16];
                    DrawTextW(
                        item.hDC,
                        &mut check,
                        &mut check_rect,
                        DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
                    );
                }
                if entry.submenu {
                    let mut arrow_rect = item.rcItem;
                    arrow_rect.left = arrow_rect.right - self.padding;
                    let mut arrow = ['›' as u16];
                    DrawTextW(
                        item.hDC,
                        &mut arrow,
                        &mut arrow_rect,
                        DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
                    );
                }
            }
            let _ = RestoreDC(item.hDC, saved);
            Some(LRESULT(1))
        }
    }
}
struct Popup(HMENU);
impl Drop for Popup {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.0);
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrayPollingAction {
    None,
    Settings,
    Read,
    Apply(PollingRate),
    Restore,
}
struct TrayPollingChoice {
    id: u16,
    text: String,
    enabled: bool,
    checked: bool,
    action: TrayPollingAction,
}
fn tray_polling_choices(
    device: &ConfigurationDevice,
    settings: &Settings,
    polling: &PollingUi,
    dashboard_pending: bool,
) -> Vec<TrayPollingChoice> {
    let choice = |id, text: String, enabled, action| TrayPollingChoice {
        id,
        text,
        enabled,
        checked: false,
        action,
    };
    if !settings.polling_controls {
        return vec![choice(
            512,
            "Enable polling controls in &Settings…".into(),
            true,
            TrayPollingAction::Settings,
        )];
    }
    let available = device.online() && polling.pending.is_none() && !dashboard_pending;
    let observed = polling
        .observations
        .get(&device.key)
        .filter(|o| o.target.device == *device && o.target.generation == polling.generation);
    let current = observed.and_then(|o| o.rate);
    let mut choices = vec![
        choice(
            0,
            if !device.online() {
                "Device is sleeping or unavailable".into()
            } else if polling.pending.is_some() || dashboard_pending {
                "Device request in progress…".into()
            } else {
                current.map_or_else(
                    || "Select a rate; hardware support is checked before changing".into(),
                    |r| format!("Last confirmed: {} Hz", r.hz()),
                )
            },
            false,
            TrayPollingAction::None,
        ),
        choice(
            510,
            "&Refresh hardware rate".into(),
            available,
            TrayPollingAction::Read,
        ),
    ];
    let rates = observed.map_or_else(
        || {
            hb_providers::controls::polling_menu_rates(device)
                .iter()
                .filter_map(|rate| PollingRate::try_from(*rate).ok())
                .collect::<Vec<_>>()
        },
        |o| o.supported.clone(),
    );
    for (index, rate) in rates.iter().take(7).enumerate() {
        let mut row = choice(
            520 + index as u16,
            format!("{} Hz", rate.hz()),
            available,
            TrayPollingAction::Apply(*rate),
        );
        row.checked = current == Some(*rate);
        choices.push(row);
    }
    if let Some(observation) = observed
        && let Some(previous) = polling.previous.get(&device.key)
        && observation.supported.contains(previous)
    {
        choices.push(choice(
            511,
            format!("Restore &previous ({} Hz)", previous.hz()),
            available && current.is_some() && current != Some(*previous),
            TrayPollingAction::Restore,
        ));
    }
    if let Some(status) = polling.status.get(&device.key) {
        let mut text: String = status.chars().take(90).collect();
        if status.chars().count() > 90 {
            text.push('…');
        }
        // Status text is data, never a keyboard mnemonic.
        choices.push(choice(
            0,
            text.replace('&', "&&"),
            false,
            TrayPollingAction::None,
        ));
    }
    choices
}
fn append_polling_menu(menu: HMENU, choices: &[TrayPollingChoice]) -> windows::core::Result<()> {
    unsafe {
        let submenu = Popup(CreatePopupMenu()?);
        for choice in choices {
            let text = wide(&choice.text);
            let flags = MF_STRING
                | if choice.enabled {
                    MF_ENABLED
                } else {
                    MF_GRAYED
                }
                | if choice.checked {
                    MF_CHECKED
                } else {
                    MF_UNCHECKED
                };
            AppendMenuW(submenu.0, flags, choice.id as usize, PCWSTR(text.as_ptr()))?;
        }
        InsertMenuW(
            menu,
            4,
            MF_BYPOSITION | MF_POPUP,
            submenu.0.0 as usize,
            w!("&Polling rate"),
        )?;
        // DestroyMenu on the parent now owns the entire submenu tree.
        std::mem::forget(submenu);
        Ok(())
    }
}
fn popup_menu(
    snapshot: &Snapshot,
    settings: &Settings,
    key: Option<&str>,
) -> windows::core::Result<Popup> {
    unsafe {
        let menu = Popup(CreatePopupMenu()?);
        if let Some(d) = key.and_then(|key| snapshot.devices.iter().find(|d| d.reading.key == key))
        {
            let text = wide(&d.text);
            let _ = AppendMenuW(menu.0, MF_STRING | MF_DISABLED, 0, PCWSTR(text.as_ptr()));
            let _ = AppendMenuW(menu.0, MF_STRING, 503, w!("Device &controls"));
            let _ = AppendMenuW(menu.0, MF_STRING, 504, w!("Battery &history"));
            let _ = AppendMenuW(menu.0, MF_STRING, 505, w!("&Hide tray icon"));
            let _ = AppendMenuW(menu.0, MF_SEPARATOR, 0, None);
        }
        if settings.fluent_menu {
            for (i, d) in snapshot
                .devices
                .iter()
                .enumerate()
                .filter(|(_, d)| !d.hidden)
            {
                let text = wide(&d.text);
                let _ = AppendMenuW(menu.0, MF_STRING, 600 + i, PCWSTR(text.as_ptr()));
            }
            let _ = AppendMenuW(menu.0, MF_SEPARATOR, 0, None);
        }
        let _ = AppendMenuW(menu.0, MF_STRING, 500, w!("Open &dashboard"));
        let _ = AppendMenuW(menu.0, MF_STRING, 501, w!("&Refresh"));
        let _ = AppendMenuW(menu.0, MF_STRING, 502, w!("E&xit Halo Battery Next"));

        Ok(menu)
    }
}

#[cfg(test)]
mod popup_tests {
    use super::*;
    fn polling_fixture() -> (ConfigurationDevice, Settings, PollingUi) {
        let mut reading = Reading::new("simulated:mouse", "Simulated mouse", "simulation", 0);
        reading.kind = "mouse".into();
        let device = ConfigurationDevice::from_reading(&reading);
        let settings = Settings {
            polling_controls: true,
            ..Default::default()
        };
        (device, settings, PollingUi::default())
    }
    #[test]
    fn tray_rate_hints_are_immediate_but_only_readback_is_checked() {
        let (device, settings, mut polling) = polling_fixture();
        let choices = tray_polling_choices(&device, &settings, &polling, false);
        assert!(
            choices
                .iter()
                .any(|c| c.enabled && c.action == TrayPollingAction::Read)
        );
        assert!(
            choices
                .iter()
                .any(|c| c.enabled && matches!(c.action, TrayPollingAction::Apply(_)))
        );
        assert!(choices.iter().all(|c| !c.checked));
        let requested = PollingRate::try_from(2000).unwrap();
        let direct = polling
            .begin_tray(
                device.clone(),
                PollingIntent::Apply {
                    rate: requested,
                    restore: false,
                },
            )
            .unwrap();
        assert_eq!(direct.action, ControlAction::Apply(requested));
        assert_eq!(direct.target.generation, polling.generation);
        assert!(polling.observations.is_empty());
        polling.abandon();
        let request = polling.begin(device.clone(), PollingIntent::Read).unwrap();
        let rate = PollingRate::try_from(1000).unwrap();
        let outcome = ControlOutcome {
            request: request.request,
            key: device.key.clone(),
            observation: Some(PollingObservation {
                target: ControlTarget {
                    device: device.clone(),
                    generation: 3,
                },
                supported: vec![rate, PollingRate::try_from(8000).unwrap()],
                rate: Some(rate),
                timestamp: 5,
                evidence: "Invented readback".into(),
            }),
            previous: None,
            may_have_changed: false,
            failure: None,
        };
        assert!(polling.accept(&outcome, Some(&device.key)));
        let choices = tray_polling_choices(&device, &settings, &polling, false);
        assert_eq!(
            choices
                .iter()
                .filter(|c| matches!(c.action, TrayPollingAction::Apply(_)))
                .count(),
            2
        );
        assert!(
            choices
                .iter()
                .any(|c| c.checked && c.action == TrayPollingAction::Apply(rate))
        );
        assert!(
            !tray_polling_choices(&device, &settings, &polling, true)
                .iter()
                .any(|c| c.enabled)
        );
        let mut sleeping = device.clone();
        sleeping.connection = Connection::Sleeping;
        assert!(
            !tray_polling_choices(&sleeping, &settings, &polling, false)
                .iter()
                .any(|c| c.enabled)
        );
        let mut replaced = device.clone();
        replaced.serial = Some("invented-replacement".into());
        assert!(
            !tray_polling_choices(&replaced, &settings, &polling, false)
                .iter()
                .any(|c| c.checked)
        );
        polling
            .previous
            .insert(device.key.clone(), PollingRate::try_from(8000).unwrap());
        assert!(
            tray_polling_choices(&device, &settings, &polling, false)
                .iter()
                .any(|c| c.enabled && c.action == TrayPollingAction::Restore)
        );
        polling.invalidate(4);
        assert!(
            !tray_polling_choices(&device, &settings, &polling, false)
                .iter()
                .any(|c| matches!(c.action, TrayPollingAction::Restore))
        );
        let disabled = tray_polling_choices(&device, &Settings::default(), &polling, false);
        assert_eq!(disabled.len(), 1);
        assert_eq!(disabled[0].action, TrayPollingAction::Settings);
    }
    #[test]
    fn revoked_polling_evidence_cannot_survive_abandoned_ui_write() {
        let (device, settings, mut polling) = polling_fixture();
        let rate = PollingRate::try_from(1000).unwrap();
        polling.observations.insert(
            device.key.clone(),
            PollingObservation {
                target: ControlTarget {
                    device: device.clone(),
                    generation: 0,
                },
                supported: vec![rate],
                rate: Some(rate),
                timestamp: 0,
                evidence: "Invented".into(),
            },
        );
        polling
            .begin(
                device.clone(),
                PollingIntent::Apply {
                    rate,
                    restore: false,
                },
            )
            .unwrap();
        polling.revoke_observation(&device.key);
        assert!(polling.pending.is_some());
        polling.abandon();
        assert!(
            !tray_polling_choices(&device, &settings, &polling, false)
                .iter()
                .any(|c| c.checked)
        );
        assert!(
            polling
                .begin(
                    device,
                    PollingIntent::Apply {
                        rate,
                        restore: false
                    }
                )
                .is_err()
        );
    }
    #[test]
    fn native_polling_submenu_keeps_ids_disabled_states_and_checkmarks() {
        let _guard = NATIVE_TEST_LOCK.lock().unwrap();
        let menu = popup_menu(&Snapshot::default(), &Settings::default(), None).unwrap();
        let rate = PollingRate::try_from(8000).unwrap();
        let choices = vec![
            TrayPollingChoice {
                id: 0,
                text: "Read first".into(),
                enabled: false,
                checked: false,
                action: TrayPollingAction::None,
            },
            TrayPollingChoice {
                id: 510,
                text: "&Refresh".into(),
                enabled: true,
                checked: false,
                action: TrayPollingAction::Read,
            },
            TrayPollingChoice {
                id: 520,
                text: "8000 Hz".into(),
                enabled: true,
                checked: true,
                action: TrayPollingAction::Apply(rate),
            },
        ];
        append_polling_menu(menu.0, &choices).unwrap();
        unsafe {
            let child = GetSubMenu(menu.0, GetMenuItemCount(Some(menu.0)) - 1);
            assert!(!child.is_invalid());
            assert_eq!(GetMenuItemCount(Some(child)), 3);
            assert_ne!(GetMenuState(child, 0, MF_BYPOSITION) & MF_GRAYED.0, 0);
            assert_eq!(GetMenuItemID(child, 1), 510);
            assert_ne!(GetMenuState(child, 520, MF_BYCOMMAND) & MF_CHECKED.0, 0);
        }
    }
    fn menu_text(menu: HMENU, index: u32) -> String {
        unsafe {
            let mut text = [0u16; 256];
            let n = GetMenuStringW(menu, index, Some(&mut text), MF_BYPOSITION);
            String::from_utf16_lossy(&text[..n as usize])
        }
    }
    #[test]
    fn selected_device_popup_reads_current_summary_and_exposes_accessible_actions() {
        let _guard = NATIVE_TEST_LOCK.lock().unwrap();
        let settings = Settings::default();
        let mut snapshot = Snapshot::default();
        let reading = Reading::new("mouse", "Mouse", "test", 0);
        snapshot.devices.push(DeviceView {
            reading,
            name: "Mouse".into(),
            icon: "mouse".into(),
            low_alert_at: 20,
            seconds_left: None,
            text: "Mouse: 70% · charging".into(),
            hidden: false,
        });
        let menu = popup_menu(&snapshot, &settings, Some("mouse")).unwrap();
        assert_eq!(menu_text(menu.0, 0), "Mouse: 70% · charging");
        unsafe {
            assert_ne!(GetMenuState(menu.0, 0, MF_BYPOSITION) & MF_DISABLED.0, 0);
            assert_eq!(GetMenuItemID(menu.0, 1), 503);
            assert!(menu_text(menu.0, 1).contains('&'));
            assert_eq!(GetMenuItemID(menu.0, 2), 504);
            assert_eq!(GetMenuItemID(menu.0, 3), 505);
        }
        drop(menu);
        snapshot.devices[0].text = "Renamed mouse: sleeping".into();
        let renamed = popup_menu(&snapshot, &settings, Some("mouse")).unwrap();
        assert_eq!(menu_text(renamed.0, 0), "Renamed mouse: sleeping");
        assert!(!menu_text(renamed.0, 0).contains('\n'));
    }
    #[test]
    fn placeholder_popup_has_no_device_controls_or_icon_picker() {
        let _guard = NATIVE_TEST_LOCK.lock().unwrap();
        let menu = popup_menu(
            &Snapshot::default(),
            &Settings::default(),
            Some("application"),
        )
        .unwrap();
        unsafe {
            for id in [503, 504, 505] {
                assert_eq!(GetMenuState(menu.0, id, MF_BYCOMMAND), u32::MAX);
            }
            assert!(GetMenuItemCount(Some(menu.0)) >= 3);
        }
    }
}

#[cfg(test)]
mod popup_theme_tests {
    use super::*;
    use windows::Win32::{
        System::Threading::{GR_GDIOBJECTS, GetGuiResources},
        UI::Controls::ODS_FLAGS,
    };
    #[test]
    fn nested_popups_register_unique_items_and_restore_native_metadata() {
        let _guard = NATIVE_TEST_LOCK.lock().unwrap();
        unsafe {
            let menu = Popup(CreatePopupMenu().unwrap());
            let submenu = CreatePopupMenu().unwrap();
            let nested = CreatePopupMenu().unwrap();
            AppendMenuW(nested, MF_STRING, 703, w!("&Refresh")).unwrap();
            AppendMenuW(submenu, MF_STRING, 702, w!("&Rate")).unwrap();
            AppendMenuW(submenu, MF_POPUP, nested.0 as usize, w!("&More")).unwrap();
            AppendMenuW(menu.0, MF_STRING, 700, w!("&Root")).unwrap();
            AppendMenuW(menu.0, MF_POPUP, submenu.0 as usize, w!("&Polling rate")).unwrap();
            let native = MENUITEMINFOW {
                cbSize: size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_DATA,
                dwItemData: 77,
                ..Default::default()
            };
            SetMenuItemInfoW(submenu, 0, true, &native).unwrap();
            let appearance =
                PopupAppearance::new(menu.0, HWND::default(), Palette::new(true, false)).unwrap();
            assert_eq!(appearance.menus.len(), 3);
            assert_eq!(appearance.items.len(), 5);
            let mut data = std::collections::BTreeSet::new();
            for (handle, _) in &appearance.menus {
                let mut background = MENUINFO {
                    cbSize: size_of::<MENUINFO>() as u32,
                    fMask: MIM_BACKGROUND,
                    ..Default::default()
                };
                GetMenuInfo(*handle, &mut background).unwrap();
                assert_eq!(background.hbrBack, appearance.background);
                for position in 0..GetMenuItemCount(Some(*handle)) as u32 {
                    let mut info = MENUITEMINFOW {
                        cbSize: size_of::<MENUITEMINFOW>() as u32,
                        fMask: MIIM_FTYPE | MIIM_DATA,
                        ..Default::default()
                    };
                    GetMenuItemInfoW(*handle, position, true, &mut info).unwrap();
                    assert_ne!(info.fType.0 & MFT_OWNERDRAW.0, 0);
                    assert!(data.insert(info.dwItemData));
                }
            }
            for handle in [menu.0, submenu, nested] {
                let result = appearance
                    .message(WM_MENUCHAR, WPARAM('r' as usize), LPARAM(handle.0 as isize))
                    .unwrap()
                    .0 as u32;
                assert_eq!(result >> 16, MNC_EXECUTE);
                assert_eq!(result & 0xffff, 0);
            }
            assert_eq!(appearance.mnemonic('m').0 as u32 >> 16, MNC_IGNORE);
            appearance.restore_native();
            let mut restored = MENUITEMINFOW {
                cbSize: size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_FTYPE | MIIM_DATA,
                ..Default::default()
            };
            GetMenuItemInfoW(submenu, 0, true, &mut restored).unwrap();
            assert_eq!(restored.dwItemData, 77);
            assert_eq!(restored.fType.0 & MFT_OWNERDRAW.0, 0);
            for (handle, original) in &appearance.menus {
                let mut background = MENUINFO {
                    cbSize: size_of::<MENUINFO>() as u32,
                    fMask: MIM_BACKGROUND,
                    ..Default::default()
                };
                GetMenuInfo(*handle, &mut background).unwrap();
                assert_eq!(background.hbrBack, *original);
            }
            drop(menu);
            drop(appearance);
        }
    }
    #[test]
    fn menu_mnemonics_skip_escaped_ampersands() {
        assert_eq!(
            menu_mnemonic(&"Open &dashboard".encode_utf16().collect::<Vec<_>>()),
            Some('d')
        );
        assert_eq!(
            menu_mnemonic(&"Mouse && keyboard".encode_utf16().collect::<Vec<_>>()),
            None
        );
        assert_eq!(
            menu_mnemonic(&"Mouse && &keyboard".encode_utf16().collect::<Vec<_>>()),
            Some('k')
        );
    }
    #[test]
    fn native_popup_palette_pixels_mnemonics_and_resources_follow_theme() {
        let _guard = NATIVE_TEST_LOCK.lock().unwrap();
        unsafe {
            for dark in [false, true] {
                let menu = popup_menu(
                    &Snapshot::default(),
                    &Settings {
                        fluent_menu: false,
                        ..Settings::default()
                    },
                    None,
                )
                .unwrap();
                let palette = Palette::new(dark, false);
                let appearance = PopupAppearance::new(menu.0, HWND::default(), palette).unwrap();
                assert_eq!(appearance.palette, palette);
                assert!(appearance.width > 100);
                assert_eq!(
                    appearance.items[0].text,
                    "Open &dashboard".encode_utf16().collect::<Vec<_>>()
                );
                let mut info = MENUITEMINFOW {
                    cbSize: size_of::<MENUITEMINFOW>() as u32,
                    fMask: MIIM_FTYPE | MIIM_DATA,
                    ..Default::default()
                };
                GetMenuItemInfoW(menu.0, 0, true, &mut info).unwrap();
                assert_ne!(info.fType.0 & MFT_OWNERDRAW.0, 0);
                assert_eq!(info.dwItemData, 1);
                let result = appearance
                    .message(WM_MENUCHAR, WPARAM('d' as usize), LPARAM(menu.0.0 as isize))
                    .unwrap()
                    .0 as u32;
                assert_eq!(result >> 16, MNC_EXECUTE);
                assert_eq!(GetMenuItemID(menu.0, (result & 0xffff) as i32), 500);
                let source = GetDC(None);
                let hdc = CreateCompatibleDC(Some(source));
                let bitmap = CreateCompatibleBitmap(
                    source,
                    appearance.width as i32,
                    appearance.row_height as i32,
                );
                let old = SelectObject(hdc, bitmap.into());
                let mut item = DRAWITEMSTRUCT {
                    CtlType: ODT_MENU,
                    hwndItem: HWND(menu.0.0),
                    hDC: hdc,
                    rcItem: RECT {
                        left: 0,
                        top: 0,
                        right: appearance.width as i32,
                        bottom: appearance.row_height as i32,
                    },
                    itemData: 1,
                    ..Default::default()
                };
                for (state, expected) in [
                    (Default::default(), palette.surface),
                    (ODS_SELECTED, palette.selection),
                    (ODS_FLAGS(ODS_SELECTED.0 | ODS_DISABLED.0), palette.surface),
                ] {
                    item.itemState = state;
                    assert_eq!(
                        appearance.message(
                            WM_DRAWITEM,
                            WPARAM(0),
                            LPARAM(&item as *const _ as isize)
                        ),
                        Some(LRESULT(1))
                    );
                    assert_eq!(GetPixel(hdc, 2, 2), expected);
                }
                SelectObject(hdc, old);
                let _ = DeleteObject(bitmap.into());
                let _ = DeleteDC(hdc);
                ReleaseDC(None, source);
                let font = appearance.font;
                let brush = appearance.background;
                assert!(GetObjectW(font.into(), 0, None) > 0);
                assert!(GetObjectW(brush.into(), 0, None) > 0);
                drop(menu);
                drop(appearance);
            }
            let menu = popup_menu(
                &Snapshot::default(),
                &Settings {
                    fluent_menu: false,
                    ..Settings::default()
                },
                None,
            )
            .unwrap();
            assert!(
                PopupAppearance::new(menu.0, HWND::default(), Palette::new(true, true)).is_none()
            );
            let mut info = MENUITEMINFOW {
                cbSize: size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_FTYPE,
                ..Default::default()
            };
            GetMenuItemInfoW(menu.0, 0, true, &mut info).unwrap();
            assert_eq!(info.fType.0 & MFT_OWNERDRAW.0, 0);
        }
    }
    #[test]
    fn repeated_popup_lifecycle_keeps_gdi_resources_bounded() {
        let _guard = NATIVE_TEST_LOCK.lock().unwrap();
        unsafe {
            let process = windows::Win32::System::Threading::GetCurrentProcess();
            let before = GetGuiResources(process, GR_GDIOBJECTS);
            for _ in 0..40 {
                let menu = popup_menu(&Snapshot::default(), &Settings::default(), None).unwrap();
                let appearance =
                    PopupAppearance::new(menu.0, HWND::default(), Palette::new(true, false))
                        .unwrap();
                assert_eq!(Rc::strong_count(&appearance), 1);
                drop(menu);
                drop(appearance);
            }
            let after = GetGuiResources(process, GR_GDIOBJECTS);
            assert!(
                after <= before + 2,
                "popup GDI resources grew: {before} -> {after}"
            );
        }
    }
    #[test]
    fn native_owner_draw_mnemonics_skip_disabled_and_cycle_duplicates() {
        let _guard = NATIVE_TEST_LOCK.lock().unwrap();
        unsafe {
            let menu = Popup(CreatePopupMenu().unwrap());
            AppendMenuW(menu.0, MF_STRING | MF_DISABLED, 1, w!("&Disabled")).unwrap();
            AppendMenuW(menu.0, MF_STRING, 2, w!("&Dashboard")).unwrap();
            AppendMenuW(menu.0, MF_STRING, 3, w!("&Devices")).unwrap();
            let appearance =
                PopupAppearance::new(menu.0, HWND::default(), Palette::new(true, false)).unwrap();
            let first = appearance.mnemonic('d').0 as u32;
            assert_eq!(first >> 16, MNC_SELECT);
            assert_eq!(first & 0xffff, 1);
            let highlight = MENUITEMINFOW {
                cbSize: size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_STATE,
                fState: MFS_HILITE,
                ..Default::default()
            };
            SetMenuItemInfoW(menu.0, 1, true, &highlight).unwrap();
            let next = appearance.mnemonic('d').0 as u32;
            assert_eq!(next & 0xffff, 2);
            assert_eq!(appearance.mnemonic('z').0 as u32 >> 16, MNC_IGNORE);
            drop(menu);
            drop(appearance);
        }
    }
}

#[cfg(test)]
mod polling_tests {
    use super::*;
    fn rate(hz: u32) -> PollingRate {
        PollingRate::try_from(hz).unwrap()
    }
    fn reading() -> ConfigurationDevice {
        ConfigurationDevice::from_reading(&Reading::new("mouse", "Mouse", "simulation", 0))
    }
    fn outcome(request: u64, generation: u64, hz: Option<u32>) -> ControlOutcome {
        ControlOutcome {
            request,
            key: "mouse".into(),
            observation: Some(PollingObservation {
                target: ControlTarget {
                    device: reading(),
                    generation,
                },
                supported: vec![rate(1000), rate(4000), rate(8000)],
                rate: hz.map(rate),
                timestamp: 0,
                evidence: "verified simulation".into(),
            }),
            previous: None,
            may_have_changed: false,
            failure: None,
        }
    }
    fn read(ui: &mut PollingUi, generation: u64, hz: u32) {
        let request = ui.begin(reading(), PollingIntent::Read).unwrap();
        assert_eq!(request.action, ControlAction::Read);
        assert_eq!(request.target.generation, 0);
        assert!(ui.accept(
            &outcome(request.request, generation, Some(hz)),
            Some("mouse")
        ));
    }
    #[test]
    fn polling_requires_verified_read_and_supported_rate() {
        assert!(!Settings::default().polling_controls);
        let mut ui = PollingUi::default();
        let intent = PollingIntent::Apply {
            rate: rate(8000),
            restore: false,
        };
        assert!(ui.begin(reading(), intent).is_err());
        read(&mut ui, 7, 1000);
        assert!(ui.pending.is_none()); // A read never schedules an Apply.
        assert!(
            ui.begin(
                reading(),
                PollingIntent::Apply {
                    rate: rate(125),
                    restore: false
                }
            )
            .is_err()
        );
        let request = ui.begin(reading(), intent).unwrap();
        assert_eq!(request.target.generation, 7);
        assert_eq!(request.action, ControlAction::Apply(rate(8000)));
        assert!(ui.begin(reading(), PollingIntent::Read).is_err());
    }
    #[test]
    fn unknown_rate_and_stale_replies_cannot_enable_apply() {
        let mut ui = PollingUi::default();
        let request = ui.begin(reading(), PollingIntent::Read).unwrap();
        assert!(!ui.accept(&outcome(request.request + 1, 3, Some(1000)), Some("mouse")));
        assert!(!ui.accept(&outcome(request.request, 3, Some(1000)), Some("other")));
        assert!(ui.accept(&outcome(request.request, 3, None), Some("mouse")));
        assert!(
            ui.begin(
                reading(),
                PollingIntent::Apply {
                    rate: rate(8000),
                    restore: false
                }
            )
            .is_err()
        );
        ui.invalidate(4);
        assert!(!ui.accept(&outcome(request.request, 3, Some(1000)), Some("mouse")));
        let request = ui.begin(reading(), PollingIntent::Read).unwrap();
        assert!(ui.accept(&outcome(request.request, 3, Some(1000)), Some("mouse")));
        assert!(ui.observations.is_empty());
    }
    #[test]
    fn restore_uses_latest_verified_before_value_and_survives_partial_failure() {
        let mut ui = PollingUi::default();
        read(&mut ui, 2, 1000);
        for (before, after) in [(1000, 8000), (8000, 4000)] {
            let req = ui
                .begin(
                    reading(),
                    PollingIntent::Apply {
                        rate: rate(after),
                        restore: false,
                    },
                )
                .unwrap();
            let mut result = outcome(req.request, 2, Some(after));
            result.previous = Some(rate(before));
            assert!(ui.accept(&result, Some("mouse")));
            assert_eq!(ui.previous["mouse"], rate(before));
        }
        let req = ui
            .begin(
                reading(),
                PollingIntent::Apply {
                    rate: rate(8000),
                    restore: false,
                },
            )
            .unwrap();
        let mut result = outcome(req.request, 2, None);
        result.previous = Some(rate(4000));
        result.failure = Some("Readback failed".into());
        result.may_have_changed = true;
        assert!(ui.accept(&result, Some("mouse")));
        assert_eq!(ui.previous["mouse"], rate(4000));
        assert!(ui.status["mouse"].contains("Hardware may have changed"));
        assert!(!ui.status["mouse"].contains("Confirmed"));
        read(&mut ui, 2, 8000);
        let req = ui
            .begin(
                reading(),
                PollingIntent::Apply {
                    rate: ui.previous["mouse"],
                    restore: true,
                },
            )
            .unwrap();
        assert_eq!(req.action, ControlAction::Apply(rate(4000)));
        let mut result = outcome(req.request, 2, Some(4000));
        result.previous = Some(rate(8000));
        ui.accept(&result, Some("mouse"));
        assert_eq!(ui.previous["mouse"], rate(4000));
        ui.invalidate(3);
        assert!(
            ui.previous.is_empty()
                && ui.status.is_empty()
                && ui.observations.is_empty()
                && ui.pending.is_none()
        );
        ui.invalidate(2);
        assert_eq!(ui.generation, 3);
    }
    #[test]
    fn closed_page_ignores_completion_and_removed_devices_release_observations() {
        let mut ui = PollingUi::default();
        read(&mut ui, 1, 1000);
        let req = ui.begin(reading(), PollingIntent::Read).unwrap();
        ui.abandon();
        assert!(!ui.accept(&outcome(req.request, 1, Some(8000)), Some("mouse")));
        assert_eq!(ui.observations["mouse"].rate, Some(rate(1000)));
        ui.retain_devices(&[]);
        assert!(ui.observations.is_empty() && ui.status.is_empty());
    }
    #[test]
    fn absolute_read_timestamp_handles_epoch_and_leap_day() {
        assert_eq!(polling_timestamp(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(polling_timestamp(-1), "1969-12-31 23:59:59 UTC");
        assert_eq!(polling_timestamp(1709210096), "2024-02-29 12:34:56 UTC");
    }
}

#[cfg(test)]
mod configuration_device_ui_tests {
    use super::*;
    fn keyboard(key: &str) -> ConfigurationDevice {
        let mut d = ConfigurationDevice::from_reading(&Reading::new(
            key,
            "Wired keyboard",
            "simulation",
            0,
        ));
        d.kind = "keyboard".into();
        d
    }
    #[test]
    fn keyboard_inventory_has_no_battery_or_tray_view_and_deduplicates_keys() {
        let keyboard = keyboard("keyboard-only");
        let rows = merged_device_rows(&[], &[keyboard.clone(), keyboard]);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].1.is_none());
        assert!(!Settings::default().polling_controls);
    }
    fn battery(key: &str) -> DeviceView {
        DeviceView {
            reading: Reading::new(key, "Mouse", "simulation", 70),
            name: "Mouse".into(),
            icon: "mouse".into(),
            low_alert_at: 20,
            seconds_left: None,
            text: "Mouse: 70%".into(),
            hidden: false,
        }
    }
    #[test]
    fn mixed_inventory_preserves_batteries_and_replaces_matching_configuration_descriptor() {
        let mouse = battery("mouse");
        let mut matched = ConfigurationDevice::from_reading(&mouse.reading);
        matched.capability = PollingCapability::Unavailable("Read unavailable".into());
        let rows = merged_device_rows(&[mouse], &[matched.clone(), keyboard("keyboard-only")]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, matched);
        assert!(rows[0].1.is_some());
        assert!(rows[1].1.is_none());
    }
    #[test]
    fn keyboard_inventory_never_changes_battery_tray_fallback() {
        let snapshot = Snapshot::default();
        let settings = Settings::default();
        let before = tray_devices(&snapshot, &settings);
        let rows = merged_device_rows(&snapshot.devices, &[keyboard("keyboard-only")]);
        assert_eq!(rows.len(), 1);
        let after = tray_devices(&snapshot, &settings);
        assert_eq!(before.len(), after.len());
        assert_eq!(
            before.first().map(|d| &d.reading.key),
            after.first().map(|d| &d.reading.key)
        );
    }
    #[test]
    fn direct_selection_and_row_indices_match_mixed_deduplicated_inventory() {
        let batteries = vec![battery("mouse"), battery("second")];
        let matched = ConfigurationDevice::from_reading(&batteries[0].reading);
        let configured = vec![
            matched,
            keyboard("keyboard"),
            keyboard("keyboard"),
            keyboard("other"),
        ];
        let rows = merged_device_rows(&batteries, &configured);
        for selected in [
            None,
            Some("missing"),
            Some("mouse"),
            Some("second"),
            Some("keyboard"),
            Some("other"),
        ] {
            let expected = selected
                .and_then(|key| rows.iter().find(|(d, _)| d.key == key))
                .unwrap_or(&rows[0]);
            let actual = current_device_row(&batteries, &configured, selected).unwrap();
            assert_eq!(actual.0, expected.0);
            assert_eq!(
                actual.1.as_ref().map(|d| &d.reading.key),
                expected.1.as_ref().map(|d| &d.reading.key)
            );
            if let Some(key) = selected {
                assert_eq!(
                    device_row_index(&batteries, &configured, key),
                    rows.iter().position(|(d, _)| d.key == key)
                );
            }
        }
        let views = tray_devices(
            &Snapshot {
                devices: batteries.clone(),
                ..Snapshot::default()
            },
            &Settings::default(),
        )
        .into_iter()
        .map(Cow::into_owned)
        .collect::<Vec<_>>();
        assert_eq!(views.len(), 2);
        let snapshot = Snapshot {
            devices: batteries,
            ..Snapshot::default()
        };
        let views = tray_devices(&snapshot, &Settings::default());
        assert!(matches!(&views[0], Cow::Borrowed(d) if std::ptr::eq(*d, &snapshot.devices[0])));
    }
    #[test]
    fn retention_uses_matching_descriptor_and_rejects_replaced_battery_identity() {
        let mut batteries = vec![battery("mouse")];
        let device = ConfigurationDevice::from_reading(&batteries[0].reading);
        let mut ui = PollingUi::default();
        ui.begin(device, PollingIntent::Read).unwrap();
        ui.retain_inventory(&batteries, &[]);
        assert!(ui.pending.is_some());
        batteries[0].reading.serial = Some("replacement".into());
        ui.retain_inventory(&batteries, &[]);
        assert!(ui.pending.is_none());
    }
    #[test]
    fn unavailable_keyboard_never_submits_hardware_request() {
        let mut device = keyboard("corsair");
        device.capability = PollingCapability::Unavailable("Unsupported protocol".into());
        let mut ui = PollingUi::default();
        assert!(ui.begin(device, PollingIntent::Read).is_err());
        assert!(ui.pending.is_none());
    }
    #[test]
    fn configuration_removal_and_identity_replacement_abandon_pending_read() {
        let device = keyboard("keyboard-only");
        let mut ui = PollingUi::default();
        ui.begin(device.clone(), PollingIntent::Read).unwrap();
        ui.retain_devices(std::slice::from_ref(&device));
        assert!(ui.pending.is_some());
        let mut replacement = device;
        replacement.serial = Some("replacement-unit".into());
        ui.retain_devices(&[replacement]);
        assert!(ui.pending.is_none());
    }
    #[test]
    fn response_from_replaced_identity_cannot_confirm_rate() {
        let mut ui = PollingUi::default();
        let request = ui
            .begin(
                ConfigurationDevice::from_reading(&Reading::new("mouse", "Mouse", "simulation", 0)),
                PollingIntent::Read,
            )
            .unwrap();
        let mut outcome = ControlOutcome {
            request: request.request,
            key: "mouse".into(),
            observation: Some(PollingObservation {
                target: request.target,
                rate: Some(PollingRate::try_from(1000).unwrap()),
                supported: vec![],
                timestamp: 0,
                evidence: "test".into(),
            }),
            previous: None,
            may_have_changed: false,
            failure: None,
        };
        outcome.observation.as_mut().unwrap().target.device.serial =
            Some("replacement-unit".into());
        assert!(ui.accept(&outcome, Some("mouse")));
        assert!(ui.observations.is_empty());
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;
    #[test]
    fn time_used_is_default_and_routes_24_hours_to_usage_query() {
        let selection = HistorySelection::default();
        assert_eq!(selection.axis, HistoryAxis::Usage);
        assert_eq!(selection.index(), 2);
        assert_eq!(selection.labels()[2], "24 hours used");
        let Command::UsageHistory {
            key,
            seconds,
            until,
            width,
            request,
        } = selection.command("mouse".into(), 123456, 640, 7)
        else {
            panic!("Usage history must not use calendar timestamps")
        };
        assert_eq!(
            (key.as_str(), seconds, until, width, request),
            ("mouse", 86400, 123456, 640, 7)
        );
        assert!(selection.description().contains("pauses"));
    }
    #[test]
    fn calendar_and_usage_keep_independent_ranges_and_route_the_requested_width() {
        let mut selection = HistorySelection::default();
        selection.set_index(0);
        selection.axis = HistoryAxis::Calendar;
        assert_eq!(selection.index(), 0);
        selection.set_index(1);
        let Command::History {
            since,
            until,
            width,
            request,
            ..
        } = selection.command("mouse".into(), 1_000_000, 333, 8)
        else {
            panic!("Calendar history must use timestamps")
        };
        assert_eq!((since, until, width, request), (395200, 1_000_000, 333, 8));
        assert!(selection.description().contains("Last known level held"));
        selection.axis = HistoryAxis::Usage;
        assert_eq!(selection.index(), 0);
        assert_eq!(selection.labels()[0], "2 hours used");
        selection.set_index(99);
        assert_eq!(selection.index(), 2);
        selection.axis = HistoryAxis::Calendar;
        assert_eq!(selection.index(), 1);
    }
}

#[cfg(test)]
mod tray_registration_tests {
    use super::*;
    fn data() -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            guidItem: GUID::from_u128(stable_guid("razer:receiver")),
            uID: 7,
            uFlags: NIF_GUID | NIF_ICON | NIF_MESSAGE | NIF_TIP,
            ..Default::default()
        }
    }
    #[derive(Default)]
    struct Recorder {
        calls: Vec<(NOTIFY_ICON_MESSAGE, GUID, u32)>,
        fail_modify: bool,
        fail_add: bool,
    }
    impl Recorder {
        fn notify(&mut self, command: NOTIFY_ICON_MESSAGE, data: &NOTIFYICONDATAW) -> bool {
            self.calls.push((command, data.guidItem, data.uID));
            !(command == NIM_MODIFY && self.fail_modify || command == NIM_ADD && self.fail_add)
        }
        fn commands(&self) -> Vec<NOTIFY_ICON_MESSAGE> {
            self.calls.iter().map(|c| c.0).collect()
        }
    }
    #[test]
    fn sleep_wake_settings_and_theme_modify_same_guid_without_recreating_icon() {
        let mut registration = TrayRegistration::default();
        let data = data();
        let mut shell = Recorder::default();
        registration.update_with(&data, TrayUpdate::Changed, true, &mut |m, d| {
            shell.notify(m, d)
        });
        // Sleep and wake change icon/tooltip, but not the Shell registration.
        for _ in 0..2 {
            registration.update_with(&data, TrayUpdate::Changed, true, &mut |m, d| {
                shell.notify(m, d)
            });
        }
        let mode = tray_message_update(WM_SETTINGCHANGE, 0xC123).unwrap();
        assert_eq!(mode, TrayUpdate::Redraw);
        registration.update_with(&data, mode, false, &mut |m, d| shell.notify(m, d));
        registration.update_with(&data, TrayUpdate::Redraw, false, &mut |m, d| {
            shell.notify(m, d)
        });
        registration.update_with(&data, TrayUpdate::Changed, false, &mut |m, d| {
            shell.notify(m, d)
        });
        assert_eq!(
            shell.commands(),
            [NIM_ADD, NIM_MODIFY, NIM_MODIFY, NIM_MODIFY, NIM_MODIFY]
        );
        assert!(
            shell
                .calls
                .iter()
                .all(|(_, guid, id)| *guid == data.guidItem && *id == 7)
        );
        registration.remove_with(&data, &mut |m, d| shell.notify(m, d));
        registration.remove_with(&data, &mut |m, d| shell.notify(m, d));
        assert_eq!(shell.commands().last(), Some(&NIM_DELETE));
        assert_eq!(
            shell
                .commands()
                .iter()
                .filter(|c| **c == NIM_DELETE)
                .count(),
            1
        );
    }
    #[test]
    fn explorer_recovery_modifies_healthy_icon_and_adds_only_after_missing_icon() {
        let mut registration = TrayRegistration::default();
        let data = data();
        let mut shell = Recorder::default();
        registration.update_with(&data, TrayUpdate::Changed, true, &mut |m, d| {
            shell.notify(m, d)
        });
        let mode = tray_message_update(0xC123, 0xC123).unwrap();
        assert_eq!(mode, TrayUpdate::ExplorerRecovery);
        registration.update_with(&data, mode, false, &mut |m, d| shell.notify(m, d));
        assert_eq!(shell.commands(), [NIM_ADD, NIM_MODIFY]);
        shell.fail_modify = true;
        registration.update_with(&data, mode, false, &mut |m, d| shell.notify(m, d));
        assert_eq!(shell.commands(), [NIM_ADD, NIM_MODIFY, NIM_MODIFY, NIM_ADD]);
        assert!(registration.registered);
        assert!(!shell.commands().contains(&NIM_DELETE));
        assert!(
            shell
                .calls
                .iter()
                .all(|(_, guid, _)| *guid == data.guidItem)
        );
    }
    #[test]
    fn ordinary_modify_failure_preserves_registration_and_failed_add_can_retry() {
        let mut registration = TrayRegistration::default();
        let data = data();
        let mut shell = Recorder {
            fail_add: true,
            ..Default::default()
        };
        registration.update_with(&data, TrayUpdate::Changed, true, &mut |m, d| {
            shell.notify(m, d)
        });
        assert!(!registration.registered);
        shell.fail_add = false;
        registration.update_with(&data, TrayUpdate::Changed, false, &mut |m, d| {
            shell.notify(m, d)
        });
        assert!(registration.registered);
        shell.fail_modify = true;
        registration.update_with(&data, TrayUpdate::Redraw, false, &mut |m, d| {
            shell.notify(m, d)
        });
        registration.update_with(&data, TrayUpdate::Changed, false, &mut |m, d| {
            shell.notify(m, d)
        });
        assert_eq!(shell.commands(), [NIM_ADD, NIM_ADD, NIM_MODIFY]);
        assert!(registration.registered);
        assert_eq!(tray_message_update(WM_NULL, 0), None);
    }
}

#[cfg(test)]
mod tooltip_polling_tests {
    use super::*;
    fn fixture() -> (DeviceView, ControlOutcome) {
        let mut reading = Reading::new("synthetic", "Test mouse", "simulation", 10);
        reading.kind = "mouse".into();
        reading.level = Some(100);
        let device = DeviceView {
            reading: reading.clone(),
            name: reading.name.clone(),
            icon: "mouse".into(),
            low_alert_at: 15,
            seconds_left: None,
            text: "Test mouse: 100%".into(),
            hidden: false,
        };
        let observation = PollingObservation {
            target: ControlTarget {
                device: ConfigurationDevice::from_reading(&reading),
                generation: 3,
            },
            supported: vec![PollingRate::try_from(2000).unwrap()],
            rate: Some(PollingRate::try_from(2000).unwrap()),
            timestamp: 10,
            evidence: "Simulation".into(),
        };
        (
            device,
            ControlOutcome {
                request: 1,
                key: reading.key,
                observation: Some(observation),
                previous: None,
                may_have_changed: false,
                failure: None,
            },
        )
    }
    fn text(buffer: &[u16]) -> String {
        String::from_utf16(buffer.split(|u| *u == 0).next().unwrap()).unwrap()
    }
    #[test]
    fn tooltip_uses_confirmed_rate_and_drops_failed_or_revoked_evidence() {
        let (device, mut outcome) = fixture();
        let mut cache = BTreeMap::new();
        let mut tip = [0u16; 128];
        cache_tooltip_rate(&mut cache, &outcome, 3, true);
        copy_tray_tooltip(&mut tip, &device, cache.get("synthetic"));
        assert_eq!(
            text(&tip),
            "Test mouse: 100%\nPolling: 2000 Hz (last confirmed)"
        );
        outcome.failure = Some("Readback failed".into());
        cache_tooltip_rate(&mut cache, &outcome, 3, true);
        copy_tray_tooltip(&mut tip, &device, cache.get("synthetic"));
        assert_eq!(text(&tip), device.text);
        outcome.failure = None;
        for (generation, enabled) in [(4, true), (3, false)] {
            cache_tooltip_rate(&mut cache, &outcome, generation, enabled);
            assert!(cache.is_empty());
        }
    }
    #[test]
    fn tooltip_reserves_rate_line_without_splitting_unicode_and_matches_identity() {
        let (mut device, outcome) = fixture();
        device.text = "🖱".repeat(128);
        let mut tip = [0u16; 128];
        copy_tray_tooltip(&mut tip, &device, outcome.observation.as_ref());
        assert_eq!(tip[127], 0);
        assert!(text(&tip).ends_with("Polling: 2000 Hz (last confirmed)"));
        device.reading.serial = Some("other-synthetic-unit".into());
        copy_tray_tooltip(&mut tip, &device, outcome.observation.as_ref());
        assert!(!text(&tip).contains("Polling:"));
        device.reading.serial = None;
        device.reading.connection = Connection::Stale;
        copy_tray_tooltip(&mut tip, &device, outcome.observation.as_ref());
        assert!(!text(&tip).contains("Polling:"));
    }
}

#[cfg(test)]
mod insights_tests {
    use super::*;
    #[test]
    fn formatting_exposes_evidence_and_estimation_limits() {
        let rate = RateInsight {
            hz: 1000,
            awake_seconds: 7200,
            projection_seconds: 7200,
            projection_consumed_percent: 20,
            projection_drop_count: 4,
            consumed_percent: 20,
            sample_count: 15,
            drop_count: 4,
            confidence: InsightConfidence::Low,
            projected_full_charge_hours: Some(10.0),
            remaining_hours: None,
        };
        let text = rate_insight_text(&rate);
        for expected in [
            "1000 Hz",
            "Low confidence",
            "2.0 h awake",
            "20 percentage points",
            "15 samples",
            "4 observed drops",
            "Estimated full-charge use: about 10 h",
            "Remaining at recent qualifying reading: Unavailable",
            "Tentative:",
        ] {
            assert!(text.contains(expected), "Missing {expected}: {text}");
        }
        assert_eq!(
            insight_estimate(Some(f64::NAN)),
            "Not enough discharge evidence"
        );
        assert_eq!(
            insight_estimate(Some(-1.0)),
            "Not enough discharge evidence"
        );
        assert!(INSIGHTS_EMPTY.contains("tray menu"));
        assert!(INSIGHTS_EMPTY.contains("Saved requested rates are never evidence"));
    }
    #[test]
    fn insight_coverage_explains_empty_rate_evidence_and_corruption() {
        let mut data = BatteryInsights::default();
        assert!(insight_empty_text(Some(&data)).contains("No retained readings"));
        data.coverage.observation_count = 12;
        assert!(insight_empty_text(Some(&data)).contains("No usable discharge readings"));
        data.coverage.discharge_sample_count = 5;
        data.coverage.awake_seconds = 3600;
        data.coverage.excluded_interval_count = 6;
        data.coverage.unreadable_row_count = 1;
        data.coverage.last_reading_timestamp = Some(0);
        assert!(insight_empty_text(Some(&data)).contains("no continuous confirmed-rate usage"));
        let text = insight_coverage_text(&data);
        for expected in [
            "12 readings",
            "5 discharging",
            "1.0 h counted",
            "0.0 h with confirmed rate",
            "6 intervals excluded",
            "1 unreadable rows",
            "1970-01-01",
        ] {
            assert!(text.contains(expected), "{text}");
        }
    }
    #[test]
    fn charge_summary_distinguishes_partial_and_inferred_evidence() {
        let mut cycle = ChargeCycle {
            start_timestamp: 0,
            end_timestamp: 3600,
            awake_seconds: 1800,
            start_percent: 70,
            end_percent: 65,
            consumed_percent: 5,
            evidence: CycleEvidence::Partial,
            current: true,
        };
        let text = charge_cycle_text(&cycle);
        assert!(text.contains("Latest discharge summary"));
        assert!(text.contains("Partial cycle"));
        assert!(text.contains("70% to 65%"));
        assert!(text.contains("0.5 h estimated awake time"));
        assert_eq!(charge_cycle_row(&cycle), "01-01 00:00 · 0.5 h");
        assert!(text.contains("Average observed drain: 10.0 percentage points/h"));
        cycle.awake_seconds = 0;
        assert!(
            charge_cycle_text(&cycle).contains("Average observed drain: Not enough awake evidence")
        );
        cycle.evidence = CycleEvidence::InferredCharge;
        assert!(charge_cycle_text(&cycle).contains("Inferred charge"));
        cycle.evidence = CycleEvidence::ObservedCharge;
        assert!(charge_cycle_text(&cycle).contains("does not imply a full charge"));
    }
    #[test]
    fn closed_or_switched_selection_ignores_insight_replies() {
        let mut ui = InsightsUi::default();
        ui.select(Some("simulation".into()));
        ui.pending = Some((1, "simulation".into()));
        assert!(!ui.accept(2, Some("simulation"), Ok(BatteryInsights::default())));
        assert!(!ui.accept(1, Some("other"), Ok(BatteryInsights::default())));
        assert!(ui.accept(1, Some("simulation"), Ok(BatteryInsights::default())));
        ui.pending = Some((2, "simulation".into()));
        ui.abandon();
        assert!(!ui.accept(2, Some("simulation"), Ok(BatteryInsights::default())));
        ui.select(Some("other".into()));
        assert!(ui.data.is_none());
    }
}

#[cfg(test)]
mod dashboard_lifecycle_tests {
    use super::*;

    unsafe fn dispatch_monitor(monitor: HWND) {
        let mut message = MSG::default();
        unsafe {
            // Scope pumping to this test's monitor. Do not consume another
            // process/window's commands or depend on timer timing.
            while PeekMessageW(
                &mut message,
                Some(monitor),
                WM_APP + 8,
                WM_APP + 11,
                PM_REMOVE,
            )
            .as_bool()
            {
                DispatchMessageW(&message);
            }
        }
    }

    #[test]
    fn native_dashboard_reopens_after_nested_close_and_external_destruction() {
        let _guard = NATIVE_TEST_LOCK.lock().unwrap();
        for restore in [false, true] {
            exercise_native_dashboard(restore);
        }
    }
    fn exercise_native_dashboard(restore: bool) {
        let mut settings = Settings {
            polling_controls: true,
            restore_polling_on_startup: restore,
            ..Settings::default()
        };
        settings
            .devices
            .entry("simulated:mouse".into())
            .or_default()
            .requested_polling_rate = Some(PollingRate::try_from(2000).unwrap());
        let test_directory = tempfile::tempdir().unwrap();
        let dir = test_directory.path().to_path_buf();
        let runtime = Runtime::start(dir.clone(), settings.clone(), true).unwrap();
        unsafe {
            let instance = GetModuleHandleW(None).unwrap();
            let class = w!("HaloBatteryNext.Native");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: instance.into(),
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
                hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as usize as *mut _),
                ..Default::default()
            };
            assert_ne!(RegisterClassW(&wc), 0);
            let context = Box::new(UiContext {
                state: RefCell::new(State {
                    context: std::ptr::null(),
                    runtime,
                    settings,
                    dir,
                    monitor: HWND::default(),
                    dashboard: None,
                    controls: BTreeMap::new(),
                    trays: BTreeMap::new(),
                    snapshot: Snapshot::default(),
                    diagnostics: BTreeMap::new(),
                    page: 1,
                    selected: 0,
                    selected_device: None,
                    configuration_devices: Vec::new(),
                    configuration_generation: 0,
                    configuration_failure: None,
                    chart: None,
                    theme: None,
                    series: HistorySeries::default(),
                    history: HistorySelection::default(),
                    request: 0,
                    taskbar: RegisterWindowMessageW(w!("TaskbarCreated")),
                    notify: None,
                    animating: false,
                    tray_theme: TrayThemeCache::default(),
                    error: String::new(),
                    font: HFONT::default(),
                    polling: PollingUi::default(),
                    tray_polling: PollingUi::default(),
                    insights: InsightsUi::default(),
                    polling_intents: BTreeMap::new(),
                    tooltip_rates: BTreeMap::new(),
                }),
                monitor: Cell::new(HWND::default()),
                paint: RefCell::new(None),
                popup: RefCell::new(None),
            });
            let ptr = &*context as *const UiContext;
            {
                let mut state = context.state.borrow_mut();
                state.context = ptr;
                state.monitor = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    class,
                    w!("Test monitor"),
                    WINDOW_STYLE::default(),
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    Some(instance.into()),
                    Some(ptr.cast()),
                )
                .unwrap();
                context.monitor.set(state.monitor);
                let deadline = Instant::now() + Duration::from_secs(5);
                while !state.tooltip_rates.contains_key("simulated:mouse")
                    && Instant::now() < deadline
                {
                    std::thread::sleep(Duration::from_millis(20));
                    state.drain();
                }
                assert!(state.tooltip_rates.contains_key("simulated:mouse"));
                assert!(state.dashboard.is_none());
                assert!(state.controls.is_empty());
                let hz = if restore { 2000 } else { 1000 };
                let expected_tip = format!("Polling: {hz} Hz (last confirmed)");
                assert_eq!(
                    state.tray_polling.previous.contains_key("simulated:mouse"),
                    restore
                );
                let tray = state.trays.get("simulated:mouse").unwrap();
                let text =
                    String::from_utf16(tray.data.szTip.split(|u| *u == 0).next().unwrap()).unwrap();
                assert!(text.contains(&expected_tip), "{text}");
                let previous_generation = state.tray_polling.generation;
                state.runtime.send(Command::Refresh);
                let deadline = Instant::now() + Duration::from_secs(5);
                while state.tray_polling.generation == previous_generation
                    && Instant::now() < deadline
                {
                    std::thread::sleep(Duration::from_millis(20));
                    state.drain();
                }
                assert!(state.tray_polling.generation > previous_generation);
                assert!(state.tray_polling.observations.is_empty());
                let tray = state.trays.get("simulated:mouse").unwrap();
                let text =
                    String::from_utf16(tray.data.szTip.split(|u| *u == 0).next().unwrap()).unwrap();
                assert!(text.contains(&expected_tip), "{text}");
                state.snapshot.devices[0].reading.connection = Connection::Sleeping;
                state.snapshot.devices[0].text = "Simulated mouse: sleeping".into();
                state.sync_trays(TrayUpdate::Changed);
                let tray = state.trays.get("simulated:mouse").unwrap();
                let text =
                    String::from_utf16(tray.data.szTip.split(|u| *u == 0).next().unwrap()).unwrap();
                assert!(text.contains(&expected_tip), "{text}");
                // Restore the empty-inventory baseline for the lifecycle scenarios below.
                state.settings = Settings::default();
                state.polling = PollingUi::default();
                state.tray_polling = PollingUi::default();
                state.snapshot = Snapshot::default();
                state.tooltip_rates.clear();
                state.sync_trays(TrayUpdate::Changed);
                state.open();
                let dashboard = state.dashboard.unwrap();
                let popup = popup_menu(&Snapshot::default(), &Settings::default(), None).unwrap();
                let appearance =
                    PopupAppearance::new(popup.0, state.monitor, Palette::new(true, false))
                        .unwrap();
                context.popup.replace(Some(appearance.clone()));
                let mut measure = MEASUREITEMSTRUCT {
                    CtlType: ODT_MENU,
                    itemData: 1,
                    ..Default::default()
                };
                assert_eq!(
                    send(
                        state.monitor,
                        WM_MEASUREITEM,
                        WPARAM(0),
                        LPARAM(&mut measure as *mut _ as isize)
                    ),
                    LRESULT(1)
                );
                assert_eq!(measure.itemWidth, appearance.width);
                let mnemonic = send(
                    state.monitor,
                    WM_MENUCHAR,
                    WPARAM('d' as usize),
                    LPARAM(popup.0.0 as isize),
                );
                assert_eq!(mnemonic.0 as u32 >> 16, MNC_EXECUTE);
                drop(popup);
                context.popup.take();
                drop(appearance);
                state.series =
                    HistorySeries::calendar(vec![Reading::new("test", "Test", "test", 0)], 0, 10);
                assert!(!state.series.samples.is_empty());
                // Synchronous close enters proc while State is borrowed: it
                // must defer rather than silently use DefWindowProc's destroy.
                send(dashboard, WM_CLOSE, WPARAM(0), LPARAM(0));
                assert!(IsWindow(Some(dashboard)).as_bool());
            }
            let monitor = context.monitor.get();
            dispatch_monitor(monitor);
            assert!(context.state.borrow().dashboard.is_none());
            assert!(context.state.borrow().controls.is_empty());
            assert!(context.state.borrow().theme.is_none());
            assert!(context.paint.borrow().is_none());
            assert!(context.state.borrow().font.is_invalid());
            {
                let mut state = context.state.borrow_mut();
                assert!(state.series.samples.is_empty());
                let request = state.request;
                state.history_outcome(
                    request,
                    Ok(HistorySeries::calendar(
                        vec![Reading::new("test", "Test", "test", 0)],
                        0,
                        10,
                    )),
                );
                assert!(state.series.samples.is_empty());
            }
            send(monitor, WM_APP + 8, WPARAM(0), LPARAM(0));
            let first = context.state.borrow().dashboard.unwrap();
            assert!(IsWindowVisible(first).as_bool());
            {
                let mut state = context.state.borrow_mut();
                state.command(2, 0);
                let request = state.request;
                let populated = || {
                    HistorySeries::calendar(vec![Reading::new("test", "Test", "test", 0)], 0, 10)
                };
                state.history_outcome(request.wrapping_sub(1), Ok(populated()));
                assert!(state.series.samples.is_empty());
                state.history_outcome(request, Ok(populated()));
                assert!(!state.series.samples.is_empty());
                state.command(1, 0);
                assert!(state.series.samples.is_empty());
                state.history_outcome(request, Ok(populated()));
                assert!(state.series.samples.is_empty());
                let current = state.request;
                state.history_outcome(current, Err(ProviderError::new("Inactive history error")));
                assert_ne!(state.error, "Inactive history error");
                state.command(2, 0);
                let reopened_request = state.request;
                state.history_outcome(reopened_request, Ok(populated()));
                assert!(!state.series.samples.is_empty());
                state.history_outcome(
                    reopened_request,
                    Err(ProviderError::new("Active history error")),
                );
                assert_eq!(state.error, "Active history error");
                state.command(1, 0);
            }
            {
                let mut state = context.state.borrow_mut();
                let mut keyboard = ConfigurationDevice::from_reading(&Reading::new(
                    "keyboard-only",
                    "Test keyboard",
                    "simulation",
                    0,
                ));
                keyboard.kind = "keyboard".into();
                keyboard.capability =
                    PollingCapability::Unavailable("Unsupported keyboard protocol".into());
                state.configuration_devices = vec![keyboard.clone(), keyboard];
                state.selected_device = Some("keyboard-only".into());
                state.build();
                assert_eq!(state.device_rows().len(), 1);
                assert!(state.controls.contains_key(&11));
                assert!(!state.controls.contains_key(&12));
                assert!(!state.controls.contains_key(&13));
                assert!(!state.controls.contains_key(&14));
                assert!(state.visible_polling_device().is_none());
                state.set_control_text(11, "Unsaved keyboard edit");
                let name_control = state.controls[&11];
                let save_control = state.controls[&15];
                let mut pending_device = state.configuration_devices[0].clone();
                pending_device.capability = PollingCapability::ReadWrite;
                state
                    .polling
                    .begin(pending_device, PollingIntent::Read)
                    .unwrap();
                let request = state.polling.pending.as_ref().unwrap().request;
                let inventory = state.configuration_devices.clone();
                state.configuration_inventory(0, inventory.clone(), None);
                assert_eq!(state.controls[&11], name_control);
                assert_eq!(state.controls[&15], save_control);
                assert_eq!(state.text(11), "Unsaved keyboard edit");
                assert_eq!(state.polling.pending.as_ref().unwrap().request, request);
                state.polling.abandon();
                state.configuration_inventory(
                    0,
                    inventory.clone(),
                    Some("Inventory failed".into()),
                );
                assert_eq!(state.error, "Inventory failed");
                state.error = "Other error".into();
                state.configuration_inventory(0, inventory.clone(), None);
                assert_eq!(state.error, "Other error");
                state.configuration_inventory(
                    0,
                    inventory.clone(),
                    Some("Inventory failed".into()),
                );
                state.configuration_inventory(0, inventory.clone(), None);
                assert!(state.error.is_empty());
                state.set_control_text(11, "Renamed keyboard");
                state.command(15, 0);
                assert_eq!(
                    state.settings.devices["keyboard-only"].name.as_deref(),
                    Some("Renamed keyboard")
                );
                assert_eq!(state.selected_device.as_deref(), Some("keyboard-only"));
                state.settings.polling_controls = true;
                state.build();
                for id in [40, 41, 42, 43] {
                    assert_ne!(
                        GetWindowLongPtrW(state.controls[&id], GWL_STYLE) as u32 & WS_DISABLED.0,
                        0
                    );
                }
                assert!(state.text(45).contains("Unsupported keyboard protocol"));
                state.command(2, 0);
                assert_eq!(
                    send(state.controls[&10], CB_GETCOUNT, WPARAM(0), LPARAM(0)).0,
                    0
                );
                assert!(state.visible_polling_device().is_none());
                state.command(6, 0);
                assert_eq!(
                    send(state.controls[&10], CB_GETCOUNT, WPARAM(0), LPARAM(0)).0,
                    0
                );
                assert!(state.visible_polling_device().is_none());
                state.command(1, 0);
                assert_eq!(state.selected_device.as_deref(), Some("keyboard-only"));
                state.command(16, 0);
                assert!(state.settings.devices["keyboard-only"].name.is_none());
                state.configuration_devices.clear();
                state.selected_device = None;
                state.settings.polling_controls = false;
                state.build();
            }
            for mode in [SW_MINIMIZE, SW_HIDE] {
                let _ = ShowWindow(first, mode);
                send(monitor, WM_APP + 8, WPARAM(0), LPARAM(0));
                assert_eq!(context.state.borrow().dashboard, Some(first));
                assert!(IsWindowVisible(first).as_bool());
                assert!(!IsIconic(first).as_bool());
            }
            {
                let mut state = context.state.borrow_mut();
                DestroyWindow(first).unwrap();
                assert_eq!(state.dashboard, Some(first)); // Deferred NCDESTROY.
                assert!(!IsWindow(Some(first)).as_bool());
                state.open(); // Must recover even before queued cleanup runs.
                assert!(state.owns_dashboard(state.dashboard.unwrap()));
            }
            dispatch_monitor(monitor);
            let reopened = context.state.borrow().dashboard.unwrap();
            assert!(context.state.borrow().owns_dashboard(reopened));
            // Native double-click and menu action must be idempotent opens.
            for _ in 0..3 {
                send(monitor, TRAY, WPARAM(0), LPARAM(WM_LBUTTONDBLCLK as isize));
                send(monitor, WM_COMMAND, WPARAM(500), LPARAM(0));
                assert_eq!(context.state.borrow().dashboard, Some(reopened));
            }
            {
                let mut state = context.state.borrow_mut();
                let mut reading = Reading::new("synthetic-mouse", "Test mouse", "razer", 0);
                reading.level = Some(50);
                state.snapshot.devices = vec![DeviceView {
                    reading,
                    name: "Test mouse".into(),
                    icon: "mouse".into(),
                    low_alert_at: 20,
                    seconds_left: None,
                    text: "Test mouse: 50%".into(),
                    hidden: false,
                }];
            }
            // Exercise each page against actual native brushes/controls without
            // changing the user's Windows appearance or stored settings.
            for dark in [false, true] {
                for page in [1, 2, 3, 6] {
                    let expected;
                    {
                        let mut state = context.state.borrow_mut();
                        state.page = page;
                        state.apply_theme(DashboardTheme::new(dark, false));
                        expected = state.theme.as_ref().unwrap().palette.background;
                        state.build();
                        if page == 1 {
                            // A combo's selected-item callback and a static's
                            // color callback reenter the parent while State is
                            // held, exactly as during a page rebuild. Print the
                            // real controls, not a direct call to the helper.
                            let palette = state.theme.as_ref().unwrap().palette;
                            for (id, background) in [
                                (10, palette.surface),
                                (91, palette.background),
                                (11, palette.surface),
                            ] {
                                let child = state.controls[&id];
                                let source = GetDC(Some(child));
                                let dc = CreateCompatibleDC(Some(source));
                                let mut rect = RECT::default();
                                GetClientRect(child, &mut rect).unwrap();
                                let bitmap =
                                    CreateCompatibleBitmap(source, rect.right, rect.bottom);
                                let old = SelectObject(dc, bitmap.into());
                                FillRect(
                                    dc,
                                    &rect,
                                    HBRUSH((COLOR_WINDOW.0 + 1) as usize as *mut _),
                                );
                                send(
                                    child,
                                    WM_PRINTCLIENT,
                                    WPARAM(dc.0 as usize),
                                    LPARAM(PRF_CLIENT as isize),
                                );
                                assert_eq!(
                                    GetPixel(dc, rect.right / 2, rect.bottom / 2),
                                    background,
                                    "reentrant control {id}, dark={dark}"
                                );
                                assert!(
                                    (3..rect.bottom - 3).any(|y| (8..rect.right / 2 - 4).any(
                                        |x| {
                                            let color = GetPixel(dc, x, y);
                                            let channel = color.0 & 255;
                                            if dark { channel > 180 } else { channel < 100 }
                                        }
                                    )),
                                    "missing reentrant control text {id}, dark={dark}"
                                );
                                SelectObject(dc, old);
                                let _ = DeleteObject(bitmap.into());
                                let _ = DeleteDC(dc);
                                ReleaseDC(Some(child), source);
                            }
                            let dc = GetDC(Some(reopened));
                            send(reopened, WM_ERASEBKGND, WPARAM(dc.0 as usize), LPARAM(0));
                            assert_eq!(
                                GetPixel(dc, 4, 4),
                                palette.background,
                                "reentrant dashboard erase, dark={dark}"
                            );
                            ReleaseDC(Some(reopened), dc);
                        }
                    }
                    let _ = RedrawWindow(
                        Some(reopened),
                        None,
                        None,
                        RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW,
                    );
                    let source = GetDC(Some(reopened));
                    let dc = CreateCompatibleDC(Some(source));
                    let mut client = RECT::default();
                    GetClientRect(reopened, &mut client).unwrap();
                    let width = client.right;
                    let height = client.bottom;
                    let bitmap = CreateCompatibleBitmap(source, width, height);
                    let old = SelectObject(dc, bitmap.into());
                    send(reopened, WM_ERASEBKGND, WPARAM(dc.0 as usize), LPARAM(0));
                    assert_eq!(
                        GetPixel(dc, width - 8, height - 8),
                        expected,
                        "page {page}, dark={dark}"
                    );
                    // Exercise the real print/client paint route, not only an
                    // explicit erase message with a forced palette.
                    FillRect(dc, &client, HBRUSH((COLOR_WINDOW.0 + 1) as usize as *mut _));
                    send(reopened, WM_PRINTCLIENT, WPARAM(dc.0 as usize), LPARAM(0));
                    assert_eq!(
                        GetPixel(dc, 0, 0),
                        expected,
                        "client paint page {page}, dark={dark}"
                    );
                    if std::env::var_os("HALO_CAPTURE_DASHBOARD_TEST").is_some() {
                        send(
                            reopened,
                            WM_PRINT,
                            WPARAM(dc.0 as usize),
                            LPARAM((PRF_CLIENT | PRF_CHILDREN | PRF_ERASEBKGND) as isize),
                        );
                        SelectObject(dc, old);
                        let mut info = BITMAPINFO::default();
                        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                        info.bmiHeader.biWidth = width;
                        info.bmiHeader.biHeight = -height;
                        info.bmiHeader.biPlanes = 1;
                        info.bmiHeader.biBitCount = 32;
                        let mut pixels = vec![0u8; width as usize * height as usize * 4];
                        assert_eq!(
                            GetDIBits(
                                dc,
                                bitmap,
                                0,
                                height as u32,
                                Some(pixels.as_mut_ptr().cast()),
                                &mut info,
                                DIB_RGB_COLORS
                            ),
                            height
                        );
                        let mut file = Vec::with_capacity(54 + pixels.len());
                        file.extend_from_slice(b"BM");
                        file.extend_from_slice(&((54 + pixels.len()) as u32).to_le_bytes());
                        file.extend_from_slice(&[0; 4]);
                        file.extend_from_slice(&54u32.to_le_bytes());
                        file.extend_from_slice(&40u32.to_le_bytes());
                        file.extend_from_slice(&width.to_le_bytes());
                        file.extend_from_slice(&(-height).to_le_bytes());
                        file.extend_from_slice(&1u16.to_le_bytes());
                        file.extend_from_slice(&32u16.to_le_bytes());
                        file.extend_from_slice(&[0; 24]);
                        file.extend_from_slice(&pixels);
                        let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                            .join("../../validation-local/dashboard");
                        std::fs::create_dir_all(&output).unwrap();
                        std::fs::write(
                            output.join(format!(
                                "page-{page}-{}.bmp",
                                if dark { "dark" } else { "light" }
                            )),
                            file,
                        )
                        .unwrap();
                    } else {
                        SelectObject(dc, old);
                    }
                    let _ = DeleteObject(bitmap.into());
                    let _ = DeleteDC(dc);
                    ReleaseDC(Some(reopened), source);
                }
            }
            {
                let mut state = context.state.borrow_mut();
                state.page = 3;
                state.build();
                let interval = state.controls[&201];
                SetWindowTextW(interval, w!("1234")).unwrap();
                for dark in [false, true, false] {
                    state.apply_theme(DashboardTheme::new(dark, false));
                    assert_eq!(state.controls[&201], interval);
                    assert_eq!(state.text(201), "1234");
                    assert_eq!(state.page, 3);
                    assert_eq!(state.dashboard, Some(reopened));
                }
            }
            // The titlebar X enters DefWindowProc(SC_CLOSE), which synchronously
            // reenters WM_CLOSE. Default processing must release State first.
            send(
                reopened,
                WM_SYSCOMMAND,
                WPARAM(SC_CLOSE as usize),
                LPARAM(0),
            );
            dispatch_monitor(monitor);
            assert!(context.state.borrow().dashboard.is_none());
            context.state.borrow_mut().runtime.stop();
            DestroyWindow(monitor).unwrap();
            UnregisterClassW(class, Some(instance.into())).unwrap();
        }
        drop(test_directory);
    }
}
