//! A single UI thread owns all HWND, HICON and Direct2D resources.
use crate::{
    chart::Chart,
    dashboard_theme::DashboardTheme,
    icons::{self, Icon},
    runtime::{Command, Event, Runtime},
};
use hb_core::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    path::PathBuf,
};
use windows::{
    Win32::{
        Devices::HumanInterfaceDevice::HidD_GetHidGuid,
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::*, Input::KeyboardAndMouse::EnableWindow, Shell::*, WindowsAndMessaging::*},
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
    signature: String,
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
    fn retain_devices(&mut self, devices: &[DeviceView]) {
        let keys = devices
            .iter()
            .take(512)
            .map(|d| d.reading.key.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        self.observations
            .retain(|key, _| keys.contains(key.as_str()));
        self.previous.retain(|key, _| keys.contains(key.as_str()));
        self.status.retain(|key, _| keys.contains(key.as_str()));
    }
    fn begin(
        &mut self,
        reading: Reading,
        intent: PollingIntent,
    ) -> Result<ControlRequest, &'static str> {
        if self.pending.is_some() {
            return Err("A device request is already pending");
        }
        let (target, action) = match intent {
            PollingIntent::Read => (
                ControlTarget {
                    reading,
                    generation: 0,
                },
                ControlAction::Read,
            ),
            PollingIntent::Apply { rate, .. } => {
                let observation = self
                    .observations
                    .get(&reading.key)
                    .ok_or("Refresh the hardware rate before applying")?;
                if observation.target.generation != self.generation
                    || observation.rate.is_none()
                    || !observation.supported.contains(&rate)
                {
                    return Err("Refresh the hardware rate and select a supported value");
                }
                (observation.target.clone(), ControlAction::Apply(rate))
            }
        };
        self.sequence = self.sequence.wrapping_add(1).max(1);
        self.pending = Some(PendingPolling {
            request: self.sequence,
            key: target.reading.key.clone(),
            intent,
        });
        self.status.insert(
            target.reading.key.clone(),
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
            if observation.target.reading.key != outcome.key
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
                self.data = Some(data);
                self.status =
                    "Local data refreshed. Select a rate or charge summary to view its evidence."
                        .into();
            }
            Err(error) => {
                self.status =
                    format!("Could not read local insights: {error}. Use Refresh to retry.")
            }
        }
        true
    }
}
const INSIGHTS_EMPTY: &str = "Collect discharge data while the device is awake. For rate comparisons, enable polling controls in Settings and manually Refresh a supported device's hardware rate under Devices. Saved requested rates are never evidence.";
fn insight_hours(seconds: u64) -> String {
    format!("{:.1} h", seconds as f64 / 3600.0)
}
fn insight_estimate(hours: Option<f64>) -> String {
    hours
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| format!("{value:.1} h"))
        .unwrap_or_else(|| "Not enough discharge evidence".into())
}
fn rate_insight_text(rate: &RateInsight) -> String {
    let confidence = match rate.confidence {
        InsightConfidence::Insufficient => "Insufficient",
        InsightConfidence::Low => "Low",
        InsightConfidence::Moderate => "Moderate",
    };
    format!(
        "{} Hz · {} confidence\r\n{} awake · {} percentage points consumed\r\n{} samples · {} observed drops\r\nEstimated full-charge use: {}\r\nRemaining at last reading: {}\r\nLast-confirmed rate; usage conditions may differ.",
        rate.hz,
        confidence,
        insight_hours(rate.awake_seconds),
        rate.consumed_percent,
        rate.sample_count,
        rate.drop_count,
        insight_estimate(rate.projected_full_charge_hours),
        rate.remaining_hours
            .filter(|hours| hours.is_finite() && *hours >= 0.0)
            .map(|hours| format!("{hours:.1} h"))
            .unwrap_or_else(|| "Unavailable".into())
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
    chart: Option<Chart>,
    theme: Option<DashboardTheme>,
    series: HistorySeries,
    history: HistorySelection,
    request: u64,
    taskbar: u32,
    notify: Option<HDEVNOTIFY>,
    animating: bool,
    error: String,
    font: HFONT,
    polling: PollingUi,
    insights: InsightsUi,
    polling_intents: BTreeMap<String, (u64, PollingRate)>,
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn copy(dst: &mut [u16], s: &str) {
    dst.fill(0);
    let x: Vec<_> = s.encode_utf16().collect();
    let n = x.len().min(dst.len() - 1);
    dst[..n].copy_from_slice(&x[..n]);
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
                chart: None,
                theme: None,
                series: HistorySeries::default(),
                history: HistorySelection::default(),
                request: 0,
                taskbar: RegisterWindowMessageW(w!("TaskbarCreated")),
                notify: None,
                animating: false,
                error: initial_error.unwrap_or_default(),
                font,
                polling: PollingUi::default(),
                insights: InsightsUi::default(),
                polling_intents: BTreeMap::new(),
            }),
            monitor: Cell::new(HWND::default()),
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
        state.runtime.send(Command::Refresh);
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
        if Some(hwnd) == s.dashboard
            && let Some(theme) = &s.theme
        {
            if let Some(result) =
                theme.control_colors(msg, HDC(wp.0 as *mut _), HWND(lp.0 as *mut _))
            {
                return result;
            }
            if msg == WM_DRAWITEM
                && let Some(result) = theme.draw_item(lp)
            {
                return result;
            }
            if msg == WM_ERASEBKGND {
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                FillRect(HDC(wp.0 as *mut _), &rect, theme.background_brush());
                return LRESULT(1);
            }
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
                BeginPaint(hwnd, &mut ps);
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
impl State {
    fn refresh_theme(&mut self) {
        self.apply_theme(DashboardTheme::new(
            hb_windows::system::dashboard_dark_theme(),
            hb_windows::system::high_contrast(),
        ));
    }
    fn apply_theme(&mut self, theme: DashboardTheme) {
        let Some(hwnd) = self.dashboard else {
            return;
        };
        theme.apply_window(hwnd);
        for control in self.controls.values() {
            theme.apply_control(*control);
        }
        self.theme = Some(theme);
        unsafe {
            let _ = RedrawWindow(
                Some(hwnd),
                None,
                None,
                RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
            );
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
        self.chart = None;
        self.theme = None;
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
        if self.dashboard.is_none() {
            return;
        }
        self.chart = None;
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
                    "Select a device to customize its tray icon and alerts",
                    20,
                    64,
                    740,
                );
                self.combo(10, &names, self.selected, 20, 96, 760);
                if let Some(d) = self.snapshot.devices.get(self.selected).cloned() {
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
                    740,
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
        unsafe {
            let _ = InvalidateRect(self.dashboard, None, true);
        }
    }
    fn set_control_text(&self, id: u16, text: &str) {
        if let Some(h) = self.controls.get(&id) {
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
            "Recent charge summaries · UTC start time (up to 10; partial cycles included)",
            20,
            340,
            760,
        );
        self.insights_list(71, 370, 165);
        self.insights_edit(76, 370, 165);
        self.control(77, w!("STATIC"),
            "Awake use is estimated device availability, not input activity. Full-charge runtime is a projection, not battery health.\r\n\r\nRates use the last confirmed setting. Refresh under Devices after changing it elsewhere. Sleep or unavailability pauses learning; reconnect, system suspend, restart or disabling controls requires fresh rate confirmation.",
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
                                .map(|hours| format!("~{hours:.1} h"))
                                .unwrap_or_else(|| "limited data".into());
                            format!("{} Hz · {estimate}", rate.hz)
                        })
                        .collect::<Vec<_>>(),
                ),
                (
                    71,
                    data.cycles.iter().take(10).map(charge_cycle_row).collect(),
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
            .unwrap_or_else(|| INSIGHTS_EMPTY.into());
        let cycle = self.insights.data.as_ref().and_then(|data| data.cycles.get(selected(71)))
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
                self.selected = self.choice(10);
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
                if let Some(d) = self.snapshot.devices.get(self.selected) {
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
                if let Some(d) = self.snapshot.devices.get(self.selected) {
                    self.settings.devices.remove(&d.reading.key);
                    self.save();
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
                    self.polling.observations.clear();
                }
                self.build()
            }
            503 => {
                self.page = 1;
                self.open();
                self.build();
            }
            504 => {
                self.page = 2;
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
                self.page = 1;
                self.open();
                self.build();
            }
            500 => self.open(),
            501 => self.runtime.send(Command::Refresh),
            502 => unsafe { PostQuitMessage(0) },
            _ => {}
        }
    }
    fn visible_polling_device(&self) -> Option<Reading> {
        (self.dashboard.is_some() && self.page == 1 && self.settings.polling_controls)
            .then(|| {
                self.snapshot
                    .devices
                    .get(self.selected)
                    .map(|d| d.reading.clone())
            })
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
                "Enable polling-rate controls in Settings to read supported devices.",
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
            || "Use Refresh rate to read this device.".into(),
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
        let pending = self.polling.pending.is_some();
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
        if let Some(reading) = self.visible_polling_device() {
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
    fn request_polling(&mut self, reading: Reading, intent: PollingIntent) {
        let key = reading.key.clone();
        match self.polling.begin(reading, intent) {
            Ok(request) => match self.runtime.submit_control(request.clone()) {
                Ok(()) => {
                    if let PollingIntent::Apply { rate, .. } = intent {
                        self.polling_intents.insert(key, (request.request, rate));
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
    fn polling_outcome(&mut self, outcome: ControlOutcome) {
        // Preserve explicit intent even when the user has left the page; never use an observation as a setting.
        if let Some(&(request, rate)) = self.polling_intents.get(&outcome.key)
            && request == outcome.request
        {
            self.polling_intents.remove(&outcome.key);
            self.settings
                .devices
                .entry(outcome.key.clone())
                .or_default()
                .requested_polling_rate = Some(rate);
            self.save();
        }
        let key = self.visible_polling_device().map(|r| r.key);
        if self.polling.accept(&outcome, key.as_deref()) {
            self.polling_controls();
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
        let mut polling_invalidated = false;
        let mut polling_selection_changed = false;
        while let Ok(e) = self.runtime.events.try_recv() {
            match e {
                Event::Snapshot(s) => latest = Some(s),
                Event::Polling(outcome) => self.polling_outcome(*outcome),
                Event::PollingInvalidated(generation) => {
                    self.polling.invalidate(generation);
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
                Event::History(id, result) if id == self.request => {
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
                _ => {}
            }
        }
        if let Some(s) = latest {
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
            self.snapshot = s;
            self.polling.retain_devices(&self.snapshot.devices);
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
                && let Some(d) = self.snapshot.devices.get(self.selected)
                && let Some(h) = self.controls.get(&91)
            {
                let text = wide(&device_detail(d));
                unsafe {
                    let _ = SetWindowTextW(*h, PCWSTR(text.as_ptr()));
                }
            }
            self.sync_trays(TrayUpdate::Changed);
            if identity != next {
                self.build()
            } else if let Some(h) = self.controls.get(&10) {
                for (i, d) in self.snapshot.devices.iter().enumerate() {
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
                    send(*h, CB_SETCURSEL, WPARAM(self.selected), LPARAM(0));
                }
            }
        }
        if polling_invalidated || polling_selection_changed {
            self.polling_controls();
            self.read_polling();
        }
    }
    fn sync_trays(&mut self, update: TrayUpdate) {
        let dark = tray_dark(&self.settings);
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
            let signature = icon_signature(d, settings, dark);
            let tip = d.text.clone();
            let existing = self.trays.get_mut(&d.reading.key);
            if let Some(t) = existing {
                let mut changed = false;
                if signature != t.signature
                    && let Ok(frames) = icons::frames(d, settings, dark, 32)
                {
                    t.frames = frames;
                    t.signature = signature;
                    t.frame = 0;
                    changed = true
                }
                let old = t.data.szTip;
                copy(&mut t.data.szTip, &tip);
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
                copy(&mut data.szTip, &tip);
                let mut registration = TrayRegistration::default();
                registration.update_with(&data, update, true, &mut shell_notify);
                self.trays.insert(
                    d.reading.key.clone(),
                    Tray {
                        registration,
                        data,
                        frames,
                        signature,
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
            drop(menu);
            if chosen.0 != 0 {
                self.command(chosen.0 as u16, 0)
            }
            let _ = PostMessageW(Some(self.monitor), WM_APP + 7, WPARAM(0), LPARAM(0));
        }
    }
}

unsafe fn send(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { SendMessageW(h, m, Some(w), Some(l)) }
}

fn icon_signature(d: &DeviceView, settings: &Settings, dark: bool) -> String {
    format!(
        "{:?}{:?}{:?}{:?}{}{}{}{:?}{:?}",
        d.reading.level,
        d.reading.precision,
        d.reading.charging,
        d.reading.connection,
        d.icon,
        d.low_alert_at,
        dark,
        settings.icon_theme,
        (
            settings.animation,
            settings.percent_in_icon,
            settings.badges,
            settings.warning_level
        )
    )
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
        let mut data = self.data;
        data.uFlags = NIF_GUID | NIF_INFO;
        copy(&mut data.szInfoTitle, &n.title);
        copy(&mut data.szInfo, &n.text);
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

fn tray_devices(snapshot: &Snapshot, settings: &Settings) -> Vec<DeviceView> {
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
        .cloned()
        .collect();
    if devices.is_empty() {
        devices.push(DeviceView {
            reading: Reading::new("application", "Halo Battery Next", "app", 0),
            name: "Halo Battery Next".into(),
            icon: "mouse".into(),
            low_alert_at: 0,
            seconds_left: None,
            text: "Halo Battery Next — no visible devices".into(),
            hidden: false,
        });
    }
    devices
}

#[cfg(test)]
mod behaviour_tests {
    use super::*;
    #[test]
    fn icon_changes_on_precision_transition_but_not_timestamp_refresh() {
        let settings = Settings::default();
        let mut d = tray_devices(&Snapshot::default(), &settings).remove(0);
        d.reading.level = Some(50);
        let exact = icon_signature(&d, &settings, false);
        d.reading.timestamp += 60;
        assert_eq!(exact, icon_signature(&d, &settings, false));
        d.reading.precision = Precision::Coarse;
        assert_ne!(exact, icon_signature(&d, &settings, false));
    }
    #[test]
    fn placeholder_exists_only_without_visible_devices() {
        let mut settings = Settings::default();
        let mut snapshot = Snapshot::default();
        let empty = tray_devices(&snapshot, &settings);
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

struct Popup(HMENU);
impl Drop for Popup {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.0);
        }
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
mod polling_tests {
    use super::*;
    fn rate(hz: u32) -> PollingRate {
        PollingRate::try_from(hz).unwrap()
    }
    fn reading() -> Reading {
        Reading::new("mouse", "Mouse", "simulation", 0)
    }
    fn outcome(request: u64, generation: u64, hz: Option<u32>) -> ControlOutcome {
        ControlOutcome {
            request,
            key: "mouse".into(),
            observation: Some(PollingObservation {
                target: ControlTarget {
                    reading: reading(),
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
mod insights_tests {
    use super::*;
    #[test]
    fn formatting_exposes_evidence_and_estimation_limits() {
        let rate = RateInsight {
            hz: 1000,
            awake_seconds: 7200,
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
            "Estimated full-charge use: 10.0 h",
            "Remaining at last reading: Unavailable",
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
        assert!(INSIGHTS_EMPTY.contains("manually Refresh"));
        assert!(INSIGHTS_EMPTY.contains("Saved requested rates are never evidence"));
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
        let settings = Settings::default();
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
                    chart: None,
                    theme: None,
                    series: HistorySeries::default(),
                    history: HistorySelection::default(),
                    request: 0,
                    taskbar: RegisterWindowMessageW(w!("TaskbarCreated")),
                    notify: None,
                    animating: false,
                    error: String::new(),
                    font: HFONT::default(),
                    polling: PollingUi::default(),
                    insights: InsightsUi::default(),
                    polling_intents: BTreeMap::new(),
                }),
                monitor: Cell::new(HWND::default()),
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
                state.open();
                let dashboard = state.dashboard.unwrap();
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
            assert!(context.state.borrow().font.is_invalid());
            send(monitor, WM_APP + 8, WPARAM(0), LPARAM(0));
            let first = context.state.borrow().dashboard.unwrap();
            assert!(IsWindowVisible(first).as_bool());
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
