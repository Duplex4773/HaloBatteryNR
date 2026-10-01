//! Engine and storage ownership. Workers move providers, never share mutable state.
use crossbeam_channel::{Receiver, Sender, bounded, select};
use hb_core::*;
use hb_windows::{BluetoothProvider, ControllerProvider, WindowsHid};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows::{
    Devices::Enumeration::{DeviceInformation, DeviceInformationUpdate, DeviceWatcher},
    Foundation::{EventHandler, TypedEventHandler},
    Gaming::Input::RawGameController,
    Win32::System::Com::*,
};
pub enum Command {
    Refresh,
    ConfigurationVisible(bool),
    ConfigurationRefresh,
    Suspend,
    Resume,
    NotificationFailed(Notification),
    Settings(Settings),
    History {
        key: String,
        since: i64,
        until: i64,
        width: usize,
        request: u64,
    },
    UsageHistory {
        key: String,
        seconds: i64,
        until: i64,
        width: usize,
        request: u64,
    },
    Insights {
        key: String,
        until: i64,
        request: u64,
    },
    Polling(ControlRequest, u64, Option<u64>),
    SettingsChanged,
    EpochSuspend(u64),
    EpochResume(u64),
    Quit,
}
pub enum Event {
    ConfigurationInventory {
        generation: u64,
        devices: Vec<ConfigurationDevice>,
        failure: Option<String>,
    },
    Polling(Box<ControlOutcome>),
    PollingInvalidated(u64),
    Snapshot(Snapshot),
    Alert(Notification),
    History(u64, Result<HistorySeries, ProviderError>),
    Insights(u64, Result<BatteryInsights, ProviderError>),
    Error(String),
    Diagnostics(BTreeMap<String, Vec<String>>),
}
pub(super) enum Storage {
    UsageSample(Vec<UsageObservation>),
    Save(Settings),
    State(Estimator),
    Status(Snapshot, bool),
    RemoveStatus,
    History(String, i64, i64, usize, u64),
    UsageHistory(String, i64, i64, usize, u64),
    Insights(String, i64, u64),
    Quit,
}
#[cfg(test)]
impl Storage {
    fn unconfirmed(readings: Vec<Reading>) -> Self {
        Self::UsageSample(
            readings
                .into_iter()
                .map(|reading| UsageObservation {
                    reading,
                    polling_rate: None,
                    session: None,
                })
                .collect(),
        )
    }
}
struct Job {
    provider: Box<dyn BatteryProvider>,
    full_mode: bool,
}
impl Job {
    fn new(provider: Box<dyn BatteryProvider>, settings: &Settings) -> Self {
        Self {
            provider,
            full_mode: settings.playstation_full_mode,
        }
    }
}
struct Completed {
    provider: Box<dyn BatteryProvider>,
    result: PollResult,
}
enum WorkerJob {
    Battery(Job),
    Polling(Box<ControlRequest>, Arc<AtomicBool>, u64, Option<u64>),
    Configuration(Arc<ConfigurationWatch>, u64, u64),
}
enum WorkerCompleted {
    Battery(Completed),
    Polling(Box<ControlOutcome>),
    Configuration(u64, u64, Result<Vec<ConfigurationDevice>, ProviderError>),
}
enum Work {
    Command(Result<Command, crossbeam_channel::RecvError>),
    ConnectionEvent,
    Completed(Result<WorkerCompleted, crossbeam_channel::RecvError>),
    Idle,
}
fn effective_interval(interval: u64, quiet: bool) -> Duration {
    Duration::from_secs(if quiet { interval.max(300) } else { interval })
}
fn provider_delay(interval: u64, quiet: bool, pending: Option<Duration>) -> Duration {
    if quiet {
        effective_interval(interval, true)
    } else {
        pending
            .unwrap_or(effective_interval(interval, false))
            .max(Duration::from_secs(1))
    }
}
fn poll_is_due(
    settings: &Settings,
    id: &str,
    due: Option<Instant>,
    now: Instant,
    paused: bool,
) -> bool {
    !paused && settings.enabled(id) && due.is_some_and(|t| t <= now)
}
fn leave_quiet_mode<'a>(
    was_quiet: bool,
    quiet: bool,
    ids: impl Iterator<Item = &'a str>,
    due: &mut BTreeMap<&'a str, Instant>,
    now: Instant,
) {
    if was_quiet && !quiet {
        for id in ids {
            due.insert(id, now);
        }
    }
}
#[derive(Clone)]
pub(super) struct Events {
    tx: Sender<Event>,
    window: Arc<AtomicUsize>,
}
impl Events {
    fn wake(&self) {
        let hwnd = self.window.load(Ordering::Acquire);
        if hwnd != 0 {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(windows::Win32::Foundation::HWND(hwnd as *mut _)),
                    windows::Win32::UI::WindowsAndMessaging::WM_APP + 7,
                    windows::Win32::Foundation::WPARAM(0),
                    windows::Win32::Foundation::LPARAM(0),
                );
            }
        }
    }
    pub(super) fn send(&self, event: Event) -> Result<(), crossbeam_channel::SendError<Event>> {
        let r = self.tx.send(event);
        if r.is_ok() {
            self.wake()
        }
        r
    }
    fn try_send(&self, event: Event) -> Result<(), crossbeam_channel::TrySendError<Event>> {
        let r = self.tx.try_send(event);
        if r.is_ok() {
            self.wake()
        }
        r
    }
}
fn update_diagnostics(
    diagnostics: &mut BTreeMap<String, Vec<String>>,
    key: String,
    value: Vec<String>,
) -> bool {
    if diagnostics.get(&key) == Some(&value) {
        return false;
    }
    diagnostics.insert(key, value);
    true
}
fn publish_diagnostics(
    events: &Events,
    diagnostics: &BTreeMap<String, Vec<String>>,
    dirty: &mut bool,
) {
    // Keep the pending update on backpressure; a later pass retries it.
    if *dirty
        && !events.tx.is_full()
        && events
            .try_send(Event::Diagnostics(diagnostics.clone()))
            .is_ok()
    {
        *dirty = false;
    }
}
pub(crate) struct ControlPermission {
    enabled: AtomicBool,
    epoch: AtomicU64,
    suspended: AtomicBool,
    desired_enabled: AtomicBool,
    acknowledged: AtomicU64,
    pending_settings: Mutex<Option<(Settings, u64)>>,
}
impl ControlPermission {
    fn new(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            epoch: AtomicU64::new(0),
            suspended: AtomicBool::new(false),
            desired_enabled: AtomicBool::new(enabled),
            acknowledged: AtomicU64::new(0),
            pending_settings: Mutex::new(None),
        }
    }
}
struct Lifecycle {
    cancel: Arc<AtomicBool>,
    permission: Arc<ControlPermission>,
    configuration: Arc<ConfigurationWatch>,
}
#[derive(Default)]
struct ConfigurationWatch {
    visible: AtomicBool,
    epoch: AtomicU64,
}
impl ConfigurationWatch {
    fn set_visible(&self, visible: bool) {
        if self.visible.swap(visible, Ordering::AcqRel) != visible {
            self.epoch.fetch_add(1, Ordering::AcqRel);
        }
    }
    fn active(&self, epoch: u64) -> bool {
        self.visible.load(Ordering::Acquire) && self.epoch.load(Ordering::Acquire) == epoch
    }
}
struct WorkerAccess {
    gates: BTreeMap<u16, Mutex<()>>,
    permission: Arc<ControlPermission>,
    configuration: Arc<ConfigurationWatch>,
}
pub struct Runtime {
    commands: Sender<Command>,
    pub events: Receiver<Event>,
    thread: Option<JoinHandle<()>>,
    permission: Arc<ControlPermission>,
    sink: Events,
    cancel: Arc<AtomicBool>,
    window: Arc<AtomicUsize>,
    configuration: Arc<ConfigurationWatch>,
}
impl Runtime {
    pub fn start(dir: PathBuf, settings: Settings, simulate: bool) -> Result<Self, ProviderError> {
        let hid = Arc::new(WindowsHid::new()?);
        let cancel = Arc::new(AtomicBool::new(false));
        let (commands, rx) = bounded(32);
        let (events, events_rx) = bounded(64);
        let cancelled = cancel.clone();
        let permission = Arc::new(ControlPermission::new(settings.polling_controls));
        let configuration = Arc::new(ConfigurationWatch::default());
        let lifecycle = Lifecycle {
            cancel: cancelled,
            permission: permission.clone(),
            configuration: configuration.clone(),
        };
        let window = Arc::new(AtomicUsize::new(0));
        let sink = Events {
            tx: events,
            window: window.clone(),
        };
        let output = sink.clone();
        let thread = thread::Builder::new()
            .name("state".into())
            .stack_size(512 * 1024)
            .spawn(move || run(dir, settings, simulate, hid, lifecycle, rx, output))?;
        Ok(Self {
            commands,
            events: events_rx,
            thread: Some(thread),
            permission,
            sink,
            cancel,
            window,
            configuration,
        })
    }
    pub fn attach_window(&self, hwnd: usize) {
        self.window.store(hwnd, Ordering::Release);
        if hwnd != 0 && !self.events.is_empty() {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(windows::Win32::Foundation::HWND(hwnd as *mut _)),
                    windows::Win32::UI::WindowsAndMessaging::WM_APP + 7,
                    windows::Win32::Foundation::WPARAM(0),
                    windows::Win32::Foundation::LPARAM(0),
                );
            }
        }
    }
    pub fn send(&self, command: Command) {
        let command = match command {
            Command::ConfigurationVisible(visible) => {
                self.configuration.set_visible(visible);
                Command::ConfigurationVisible(visible)
            }
            Command::Settings(settings) => {
                self.permission.enabled.store(false, Ordering::Release);
                let epoch = self.permission.epoch.fetch_add(1, Ordering::AcqRel) + 1;
                self.permission
                    .desired_enabled
                    .store(settings.polling_controls, Ordering::Release);
                *self
                    .permission
                    .pending_settings
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = Some((settings, epoch));
                Command::SettingsChanged
            }
            Command::Suspend => {
                self.permission.suspended.store(true, Ordering::Release);
                self.permission.enabled.store(false, Ordering::Release);
                let epoch = self.permission.epoch.fetch_add(1, Ordering::AcqRel) + 1;
                Command::EpochSuspend(epoch)
            }
            Command::Resume => {
                self.permission.suspended.store(false, Ordering::Release);
                self.permission.enabled.store(false, Ordering::Release);
                let epoch = self.permission.epoch.fetch_add(1, Ordering::AcqRel) + 1;
                Command::EpochResume(epoch)
            }
            command => command,
        };
        if self.commands.try_send(command).is_err() {
            let _ = self.sink.try_send(Event::Error("Command queue is busy; retry the action. Device configuration remains revoked until settings or resume is acknowledged.".into()));
        }
    }
    pub fn submit_control(&self, request: ControlRequest) -> Result<(), ProviderError> {
        let epoch = self.permission.epoch.load(Ordering::Acquire);
        if !self.permission.enabled.load(Ordering::Acquire)
            || !self.permission.desired_enabled.load(Ordering::Acquire)
            || self.permission.suspended.load(Ordering::Acquire)
            || self.permission.acknowledged.load(Ordering::Acquire) != epoch
        {
            return Err(ProviderError::new(
                "Polling controls are disabled or awaiting settings/resume acknowledgment; retry Refresh",
            ));
        }
        let configuration_epoch = (request.target.device.kind == "keyboard")
            .then(|| self.configuration.epoch.load(Ordering::Acquire));
        if configuration_epoch.is_some_and(|epoch| !self.configuration.active(epoch)) {
            return Err(ProviderError::new(
                "Open Devices before reading or configuring a keyboard",
            ));
        }
        self.commands
            .try_send(Command::Polling(request, epoch, configuration_epoch))
            .map_err(|e| ProviderError::new(format!("Configuration request rejected: {e}")))
    }
    pub fn stop(&mut self) {
        if self.thread.is_none() {
            return;
        }
        self.cancel.store(true, Ordering::Relaxed);
        self.configuration.set_visible(false);
        let _ = self.commands.try_send(Command::Quit);
        if let Some(t) = self.thread.take() {
            while !t.is_finished() {
                while self.events.try_recv().is_ok() {}
                thread::sleep(Duration::from_millis(10));
            }
            let _ = t.join();
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop()
    }
}
fn relevant_device_event(id: &str) -> bool {
    let id = id.to_ascii_uppercase();
    id.contains("BTH") || id.contains("HID#") || id.contains("BLUETOOTH")
}
struct DeviceEvents {
    watcher: DeviceWatcher,
    added: Option<i64>,
    updated: Option<i64>,
    removed: Option<i64>,
}
impl DeviceEvents {
    fn new(commands: &Sender<Command>) -> Option<Self> {
        let watcher = DeviceInformation::CreateWatcher().ok()?;
        let tx = commands.clone();
        let added = watcher
            .Added(&TypedEventHandler::<_, DeviceInformation>::new(
                move |_, args| {
                    if args
                        .as_ref()
                        .and_then(|a| a.Id().ok())
                        .is_some_and(|id| relevant_device_event(&id.to_string()))
                    {
                        let _ = tx.try_send(Command::Refresh);
                    }
                    Ok(())
                },
            ))
            .ok();
        let tx = commands.clone();
        let updated = watcher
            .Updated(&TypedEventHandler::<_, DeviceInformationUpdate>::new(
                move |_, args| {
                    if args
                        .as_ref()
                        .and_then(|a| a.Id().ok())
                        .is_some_and(|id| relevant_device_event(&id.to_string()))
                    {
                        let _ = tx.try_send(Command::Refresh);
                    }
                    Ok(())
                },
            ))
            .ok();
        let tx = commands.clone();
        let removed = watcher
            .Removed(&TypedEventHandler::<_, DeviceInformationUpdate>::new(
                move |_, args| {
                    if args
                        .as_ref()
                        .and_then(|a| a.Id().ok())
                        .is_some_and(|id| relevant_device_event(&id.to_string()))
                    {
                        let _ = tx.try_send(Command::Refresh);
                    }
                    Ok(())
                },
            ))
            .ok();
        let events = Self {
            watcher,
            added,
            updated,
            removed,
        };
        events.watcher.Start().ok()?;
        Some(events)
    }
}
impl Drop for DeviceEvents {
    fn drop(&mut self) {
        let _ = self.watcher.Stop();
        if let Some(token) = self.added.take() {
            let _ = self.watcher.RemoveAdded(token);
        }
        if let Some(token) = self.updated.take() {
            let _ = self.watcher.RemoveUpdated(token);
        }
        if let Some(token) = self.removed.take() {
            let _ = self.watcher.RemoveRemoved(token);
        }
    }
}
fn worker(
    jobs: Receiver<WorkerJob>,
    results: Sender<WorkerCompleted>,
    hid: Arc<WindowsHid>,
    cancel: Arc<AtomicBool>,
    winrt: bool,
    commands: Sender<Command>,
    access: Arc<WorkerAccess>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name(if winrt { "winrt" } else { "hid" }.into())
        .stack_size(512 * 1024)
        .spawn(move || {
            let _apartment = Apartment::new(winrt);
            let _device_events = if winrt {
                DeviceEvents::new(&commands)
            } else {
                None
            };
            let controller_token = if winrt {
                let tx = commands.clone();
                RawGameController::RawGameControllerAdded(&EventHandler::new(move |_, _| {
                    let _ = tx.try_send(Command::Refresh);
                    Ok(())
                }))
                .ok()
            } else {
                None
            };
            let removed_token = if winrt {
                RawGameController::RawGameControllerRemoved(&EventHandler::new(move |_, _| {
                    let _ = commands.try_send(Command::Refresh);
                    Ok(())
                }))
                .ok()
            } else {
                None
            };
            let clock = SystemClock::default();
            while let Ok(job) = jobs.recv() {
                // A vendor gate conservatively covers every physical receiver
                // used by a protocol family, including multi-collection sessions.
                // It prevents interleaving shared-vendor protocols without locking
                // individual opens (Logitech opens both long and short channels).
                let vendors: BTreeSet<_> = match &job {
                    WorkerJob::Battery(job) => hb_providers::catalog::DEVICES
                        .iter()
                        .filter(|d| d.provider == job.provider.id())
                        .map(|d| d.vid)
                        .collect(),
                    WorkerJob::Polling(request, _, _, _) => {
                        control_vendors(&request.target.device.source)
                            .into_iter()
                            .collect()
                    }
                    WorkerJob::Configuration(_, _, _) => {
                        hb_providers::configuration::CONFIGURATION_VENDORS
                            .iter()
                            .copied()
                            .collect()
                    }
                };
                let _guards = vendors
                    .iter()
                    .filter_map(|v| access.gates.get(v))
                    .map(|m| m.lock().unwrap_or_else(|p| p.into_inner()))
                    .collect::<Vec<_>>();
                let completed = match job {
                    WorkerJob::Battery(job) => {
                        WorkerCompleted::Battery(execute_job(job, &*hid, &clock, &cancel))
                    }
                    WorkerJob::Polling(request, request_cancel, epoch, configuration_epoch) => {
                        WorkerCompleted::Polling(Box::new(execute_control_inner(
                            &request,
                            &*hid,
                            &clock,
                            &cancel,
                            &request_cancel,
                            hb_windows::system::polling_apply_blocked(),
                            ControlGuards {
                                permission: Some((access.permission.clone(), epoch)),
                                configuration: configuration_epoch
                                    .map(|epoch| (access.configuration.clone(), epoch)),
                            },
                        )))
                    }
                    WorkerJob::Configuration(watch, epoch, generation) => {
                        WorkerCompleted::Configuration(
                            epoch,
                            generation,
                            discover_configuration(
                                &*hid, &clock, &cancel, &watch, epoch, generation,
                            ),
                        )
                    }
                };
                if results.send(completed).is_err() {
                    break;
                }
            }
            if let Some(t) = controller_token {
                let _ = RawGameController::RemoveRawGameControllerAdded(t);
            }
            if let Some(t) = removed_token {
                let _ = RawGameController::RemoveRawGameControllerRemoved(t);
            }
        })
        .expect("create I/O worker")
}
fn control_vendors(source: &str) -> Vec<u16> {
    match source {
        "razer" => vec![0x1532],
        "logitech" => vec![0x046d],
        "mchose" => vec![0x3837],
        "corsair" => vec![0x1b1c],
        _ => Vec::new(),
    }
}
struct InventoryTransport<'a> {
    hid: &'a dyn HidTransport,
    watch: &'a ConfigurationWatch,
    epoch: u64,
    generation: u64,
    context: &'a PollContext<'a>,
}
impl InventoryTransport<'_> {
    fn active(&self) -> Result<(), ProviderError> {
        if self.watch.active(self.epoch)
            && self.context.active()
            && self.hid.generation() == self.generation
        {
            Ok(())
        } else {
            Err(ProviderError::new(
                "configuration inventory cancelled or connection changed",
            ))
        }
    }
}
impl HidTransport for InventoryTransport<'_> {
    fn generation(&self) -> u64 {
        self.hid.generation()
    }
    fn enumerate(&self, vendor: u16) -> Result<Vec<HidInfo>, ProviderError> {
        self.active()?;
        let result = self.hid.enumerate(vendor);
        self.active()?;
        result
    }
    fn open(&self, _: &HidInfo) -> Result<Box<dyn HidSession>, ProviderError> {
        Err(ProviderError::new(
            "configuration discovery cannot open device sessions",
        ))
    }
}
fn discover_configuration(
    hid: &dyn HidTransport,
    clock: &dyn Clock,
    cancel: &AtomicBool,
    watch: &ConfigurationWatch,
    epoch: u64,
    generation: u64,
) -> Result<Vec<ConfigurationDevice>, ProviderError> {
    let context = PollContext {
        clock,
        cancelled: cancel,
        deadline: clock.monotonic() + Duration::from_secs(12),
        playstation_full_mode: false,
    };
    let transport = InventoryTransport {
        hid,
        watch,
        epoch,
        generation,
        context: &context,
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        transport.active()?;
        hb_providers::configuration::discover_keyboards(&transport, &context)
    }))
    .unwrap_or_else(|_| {
        Err(ProviderError::new(
            "configuration discovery failed; refresh to retry",
        ))
    })
}
fn configuration_matches(target: &ConfigurationDevice, observed: &ConfigurationDevice) -> bool {
    target.key == observed.key
        && target.source == observed.source
        && target.kind == observed.kind
        && target.via == observed.via
        && target.serial == observed.serial
        && target.container == observed.container
        && target.capability == observed.capability
        && observed.online()
}
fn simulated_keyboards() -> Vec<ConfigurationDevice> {
    [
        (
            "razer:026c:SIMULATED-KEYBOARD",
            "Razer Huntsman V2",
            "razer",
            PollingCapability::ReadWrite,
        ),
        (
            "corsair:1bb3:SIMULATED-CORSAIR",
            "Corsair K70 RGB Pro",
            "corsair",
            PollingCapability::Unavailable(hb_providers::configuration::CORSAIR_UNAVAILABLE.into()),
        ),
    ]
    .into_iter()
    .map(|(key, name, source, capability)| ConfigurationDevice {
        key: key.into(),
        name: name.into(),
        kind: "keyboard".into(),
        source: source.into(),
        via: "usb".into(),
        connection: Connection::Online,
        serial: Some(key.into()),
        container: Some(format!("simulation:{source}")),
        capability,
    })
    .collect()
}
fn execute_job(
    mut job: Job,
    hid: &dyn HidTransport,
    clock: &dyn Clock,
    cancel: &AtomicBool,
) -> Completed {
    let context = PollContext {
        clock,
        cancelled: cancel,
        deadline: clock.monotonic() + Duration::from_secs(25),
        playstation_full_mode: job.full_mode,
    };
    let result = if cancel.load(Ordering::Relaxed) {
        Err(ProviderError::new("cancelled"))
    } else {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            job.provider.poll(hid, &context)
        }))
        .unwrap_or_else(|_| {
            Err(ProviderError::new(
                "provider panicked; next refresh will retry",
            ))
        })
    };
    Completed {
        provider: job.provider,
        result,
    }
}
// Cancellation is checked before every native HID exchange, including jobs that
// were already queued when settings, suspend or shutdown revoked permission.
struct ControlTransport<'a> {
    hid: &'a dyn HidTransport,
    shutdown: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    permission: Option<(Arc<ControlPermission>, u64)>,
    configuration: Option<(Arc<ConfigurationWatch>, u64)>,
    block_while_gaming: bool,
}
struct ControlSession {
    inner: Box<dyn HidSession>,
    shutdown: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    permission: Option<(Arc<ControlPermission>, u64)>,
    configuration: Option<(Arc<ConfigurationWatch>, u64)>,
    block_while_gaming: bool,
    notification_state: fn() -> bool,
}
impl ControlSession {
    fn active(&self) -> Result<(), ProviderError> {
        if self.shutdown.load(Ordering::Relaxed) || self.cancelled.load(Ordering::Relaxed) {
            Err(ProviderError::new("configuration cancelled"))
        } else if self
            .configuration
            .as_ref()
            .is_some_and(|(watch, epoch)| !watch.active(*epoch))
        {
            Err(ProviderError::new(
                "Keyboard configuration cancelled: Devices is no longer visible",
            ))
        } else if self.permission.as_ref().is_some_and(|(p, epoch)| {
            !p.enabled.load(Ordering::Acquire)
                || p.suspended.load(Ordering::Acquire)
                || !p.desired_enabled.load(Ordering::Acquire)
                || p.acknowledged.load(Ordering::Acquire) != *epoch
                || p.epoch.load(Ordering::Acquire) != *epoch
        }) {
            Err(ProviderError::new("configuration permission revoked"))
        } else if self.block_while_gaming && (self.notification_state)() {
            Err(ProviderError::new(
                "Close the game or presentation and verify the Windows notification state before changing the rate",
            ))
        } else {
            Ok(())
        }
    }
}
impl HidSession for ControlSession {
    fn write(&mut self, data: &[u8]) -> Result<(), ProviderError> {
        self.active()?;
        self.inner.write(data)
    }
    fn read(&mut self, length: usize, timeout: Duration) -> Result<Vec<u8>, ProviderError> {
        self.active()?;
        self.inner.read(length, timeout)
    }
    fn send_feature(&mut self, data: &[u8]) -> Result<(), ProviderError> {
        self.active()?;
        self.inner.send_feature(data)
    }
    fn feature(&mut self, id: u8, length: usize) -> Result<Vec<u8>, ProviderError> {
        self.active()?;
        self.inner.feature(id, length)
    }
}
impl HidTransport for ControlTransport<'_> {
    fn generation(&self) -> u64 {
        self.hid.generation()
    }
    fn enumerate(&self, vendor: u16) -> Result<Vec<HidInfo>, ProviderError> {
        if self.shutdown.load(Ordering::Relaxed) || self.cancelled.load(Ordering::Relaxed) {
            return Err(ProviderError::new("configuration cancelled"));
        }
        if self
            .configuration
            .as_ref()
            .is_some_and(|(watch, epoch)| !watch.active(*epoch))
        {
            return Err(ProviderError::new(
                "Keyboard configuration cancelled: Devices is no longer visible",
            ));
        }
        self.hid.enumerate(vendor)
    }
    fn open(&self, info: &HidInfo) -> Result<Box<dyn HidSession>, ProviderError> {
        if self.shutdown.load(Ordering::Relaxed) || self.cancelled.load(Ordering::Relaxed) {
            return Err(ProviderError::new("configuration cancelled"));
        }
        if self
            .configuration
            .as_ref()
            .is_some_and(|(watch, epoch)| !watch.active(*epoch))
        {
            return Err(ProviderError::new(
                "Keyboard configuration cancelled: Devices is no longer visible",
            ));
        }
        Ok(Box::new(ControlSession {
            inner: self.hid.open(info)?,
            shutdown: self.shutdown.clone(),
            cancelled: self.cancelled.clone(),
            permission: self.permission.clone(),
            configuration: self.configuration.clone(),
            block_while_gaming: self.block_while_gaming,
            notification_state: hb_windows::system::polling_apply_blocked,
        }))
    }
}
pub(crate) fn execute_control(
    request: &ControlRequest,
    hid: &dyn HidTransport,
    clock: &dyn Clock,
    shutdown: &Arc<AtomicBool>,
    cancelled: &Arc<AtomicBool>,
    gaming: bool,
    permission: Option<(Arc<ControlPermission>, u64)>,
) -> ControlOutcome {
    execute_control_inner(
        request,
        hid,
        clock,
        shutdown,
        cancelled,
        gaming,
        ControlGuards {
            permission,
            configuration: None,
        },
    )
}
#[derive(Default)]
struct ControlGuards {
    permission: Option<(Arc<ControlPermission>, u64)>,
    configuration: Option<(Arc<ConfigurationWatch>, u64)>,
}
fn execute_control_inner(
    request: &ControlRequest,
    hid: &dyn HidTransport,
    clock: &dyn Clock,
    shutdown: &Arc<AtomicBool>,
    cancelled: &Arc<AtomicBool>,
    gaming: bool,
    guards: ControlGuards,
) -> ControlOutcome {
    if shutdown.load(Ordering::Relaxed) || cancelled.load(Ordering::Relaxed) {
        return ControlOutcome::failed(request, "Configuration cancelled");
    }
    // Uses Shell's public notification state only; no game-process inspection.
    if gaming && matches!(request.action, ControlAction::Apply(_)) {
        return ControlOutcome::failed(
            request,
            "Close the game or presentation and verify the Windows notification state before changing the rate",
        );
    }
    let context = PollContext {
        clock,
        cancelled,
        deadline: clock.monotonic() + Duration::from_secs(12),
        playstation_full_mode: false,
    };
    let transport = ControlTransport {
        hid,
        shutdown: shutdown.clone(),
        cancelled: cancelled.clone(),
        permission: guards.permission,
        configuration: guards.configuration,
        block_while_gaming: matches!(request.action, ControlAction::Apply(_)),
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        hb_providers::controls::HidDeviceController.execute(request, &transport, &context)
    }))
    .unwrap_or_else(|_| {
        ControlOutcome::failed(
            request,
            "Configuration worker failed; device state is unknown. Refresh before retrying",
        )
    })
}
fn simulate_control(
    request: &ControlRequest,
    current: &mut PollingRate,
    generation: u64,
    timestamp: i64,
) -> ControlOutcome {
    if let PollingCapability::Unavailable(reason) = &request.target.device.capability {
        return ControlOutcome::failed(request, reason.clone());
    }
    if matches!(request.action, ControlAction::Apply(_)) && request.target.generation != generation
    {
        return ControlOutcome::failed(request, "Connection changed; refresh before applying");
    }
    let supported = if request.target.device.kind == "keyboard" {
        vec![125, 250, 500, 1000, 2000, 4000, 8000]
    } else {
        vec![125, 500, 1000, 2000, 4000, 8000]
    };
    let previous = *current;
    if let ControlAction::Apply(rate) = request.action {
        if !supported.contains(&rate.hz()) {
            return ControlOutcome::failed(request, "Unsupported rate for simulated device");
        }
        *current = rate;
    }
    let mut target = request.target.clone();
    target.generation = generation;
    ControlOutcome {
        request: request.request,
        key: target.device.key.clone(),
        observation: Some(PollingObservation {
            target,
            supported: supported
                .into_iter()
                .map(|r| PollingRate::try_from(r).unwrap())
                .collect(),
            rate: Some(*current),
            timestamp,
            evidence: "Simulation; no hardware accessed".into(),
        }),
        previous: Some(previous),
        may_have_changed: previous != *current,
        failure: None,
    }
}
/// A healthy refresh can confirm a change after a previous partial SET.
fn changed_configuration(
    outcome: &ControlOutcome,
    observed: &mut BTreeMap<String, PollingRate>,
) -> bool {
    if observed.len() >= 512 && !observed.contains_key(&outcome.key) {
        observed.pop_first();
    }
    if let Some(previous) = outcome.previous {
        observed.entry(outcome.key.clone()).or_insert(previous);
    }
    if outcome.failure.is_some() {
        return false;
    }
    let Some(rate) = outcome.observation.as_ref().and_then(|o| o.rate) else {
        return false;
    };
    let changed = observed
        .insert(outcome.key.clone(), rate)
        .is_some_and(|before| before != rate);
    changed || outcome.confirmed_change()
}
/// Session-scoped readback evidence, never restored from requested settings.
struct UsageTracker {
    session: u64,
    generation: u64,
    enabled: bool,
    rates: BTreeMap<String, PollingObservation>,
    wall_clock: Option<i64>,
}
impl UsageTracker {
    fn new(generation: u64, enabled: bool) -> Self {
        let session = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros()
            .min(i64::MAX as u128 - 1) as u64;
        Self {
            session,
            generation,
            enabled,
            rates: BTreeMap::new(),
            wall_clock: None,
        }
    }
    fn invalidate(&mut self, generation: u64, enabled: bool) {
        self.session = self.session.saturating_add(1);
        self.generation = generation;
        self.enabled = enabled;
        self.rates.clear();
    }
    fn synchronize(&mut self, generation: u64, enabled: bool) {
        if generation != self.generation || enabled != self.enabled {
            self.invalidate(generation, enabled);
        }
    }
    fn observe(&mut self, outcome: &ControlOutcome) {
        // A failed/partial exchange cannot establish the resulting rate.
        let previous = self.rates.remove(&outcome.key);
        let next = outcome.observation.as_ref().and_then(|o| o.rate);
        if outcome.failure.is_some() || previous.as_ref().and_then(|o| o.rate) != next {
            self.session = self.session.saturating_add(1);
        }
        if self.enabled
            && outcome.failure.is_none()
            && let Some(observation) = &outcome.observation
            && observation.target.generation == self.generation
            && observation.rate.is_some()
            && observation.target.device.key == outcome.key
        {
            if self.rates.len() >= 512 {
                self.rates.pop_first();
            }
            self.rates.insert(outcome.key.clone(), observation.clone());
        }
    }
    fn synchronize_clock(&mut self, now: i64) {
        if self.wall_clock.is_some_and(|previous| now < previous) {
            self.invalidate(self.generation, self.enabled);
        }
        self.wall_clock = Some(now);
    }
    fn samples(&mut self, readings: Vec<Reading>) -> Vec<UsageObservation> {
        self.rates
            .retain(|key, _| readings.iter().any(|r| &r.key == key));
        readings
            .into_iter()
            .map(|reading| {
                let polling_rate = self
                    .rates
                    .get(&reading.key)
                    .filter(|o| {
                        let target = &o.target.device;
                        reading.timestamp >= o.timestamp
                            && reading.source == target.source
                            && reading.via == target.via
                            && reading.serial == target.serial
                            && reading.container == target.container
                    })
                    .and_then(|o| o.rate);
                UsageObservation {
                    reading,
                    polling_rate,
                    session: Some(self.session),
                }
            })
            .collect()
    }
}
struct Apartment(bool);
impl Apartment {
    fn new(enabled: bool) -> Self {
        Self(enabled && unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                CoUninitialize();
            }
        }
    }
}
fn run(
    dir: PathBuf,
    settings: Settings,
    simulate: bool,
    hid: Arc<WindowsHid>,
    lifecycle: Lifecycle,
    commands: Receiver<Command>,
    events: Events,
) {
    let Lifecycle {
        cancel,
        permission,
        configuration,
    } = lifecycle;
    let simulate_keyboards = simulate && std::env::args().any(|a| a == "--simulate-keyboards");
    let (storage, store_rx) = bounded(32);
    let (boot, boot_rx) = bounded(1);
    let store_events = events.clone();
    let folder = dir.clone();
    let store_thread = thread::Builder::new()
        .name("storage".into())
        .stack_size(512 * 1024)
        .spawn(move || crate::storage_worker::run(folder, store_rx, boot, store_events))
        .expect("create storage worker");
    let mut engine = Engine::new(settings, boot_rx.recv().unwrap_or_default());
    if !engine.settings.status_file {
        let _ = storage.send(Storage::RemoveStatus);
    }
    let clock = SystemClock::default();
    let (jobs, job_rx) = bounded(2);
    let (winrt, winrt_rx) = bounded(2);
    let (results, result_rx) = bounded(4);
    let (event_tx, event_rx) = bounded(8);
    let mut workers = Vec::new();
    let gates = Arc::new(WorkerAccess {
        permission: permission.clone(),
        configuration: configuration.clone(),
        gates: hb_providers::catalog::DEVICES
            .iter()
            .map(|d| d.vid)
            .chain(
                hb_providers::configuration::CONFIGURATION_VENDORS
                    .iter()
                    .copied(),
            )
            .chain([0x1532, 0x046d, 0x3837, 0x1b1c])
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|v| (v, Mutex::new(())))
            .collect::<BTreeMap<_, _>>(),
    });
    for _ in 0..2 {
        workers.push(worker(
            job_rx.clone(),
            results.clone(),
            hid.clone(),
            cancel.clone(),
            false,
            event_tx.clone(),
            gates.clone(),
        ))
    }
    workers.push(worker(
        winrt_rx,
        results.clone(),
        hid.clone(),
        cancel.clone(),
        true,
        event_tx,
        gates,
    ));
    drop(results);
    let mut providers: BTreeMap<&str, Box<dyn BatteryProvider>> = hb_providers::providers()
        .into_iter()
        .map(|p| (p.id(), p))
        .collect();
    providers.insert("bluetooth", Box::new(BluetoothProvider::default()));
    providers.insert("xinput", Box::new(ControllerProvider));
    let mut inflight = 0usize;
    let mut due: BTreeMap<&str, Instant> =
        providers.keys().map(|id| (*id, Instant::now())).collect();
    let mut invalidated = BTreeSet::new();
    let mut diagnostics = BTreeMap::new();
    let mut diagnostics_dirty = true;
    let mut control_cancel = Arc::new(AtomicBool::new(!engine.settings.polling_controls));
    let mut simulated_rates = BTreeMap::new();
    let mut configuration_devices: Vec<ConfigurationDevice> = Vec::new();
    let mut configuration_generation = hid.generation();
    let mut configuration_valid = false;
    let mut configuration_failure = None;
    let mut configuration_visible = false;
    let mut configuration_inflight = false;
    let mut configuration_due = Instant::now();
    let mut configuration_dirty = false;
    let mut observed_rates = BTreeMap::new();
    let mut usage_tracker = UsageTracker::new(hid.generation(), engine.settings.polling_controls);
    let _ = events.send(Event::PollingInvalidated(hid.generation()));
    let mut stop = false;
    let mut suspended = false;
    let mut was_quiet = false;
    let mut failed_delivery: BTreeMap<(String, NotificationKind), (Instant, Notification)> =
        BTreeMap::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            stop = true;
            control_cancel.store(true, Ordering::Relaxed);
        }
        let latest_settings = permission
            .pending_settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some((s, epoch)) = latest_settings {
            let remove = engine.settings.status_file && !s.status_file;
            control_cancel.store(true, Ordering::Relaxed);
            control_cancel = Arc::new(AtomicBool::new(!s.polling_controls));
            engine.update_settings(s.clone());
            let _ = storage.send(Storage::Save(s));
            if remove {
                let _ = storage.send(Storage::RemoveStatus);
            }
            for id in providers.keys() {
                due.insert(id, Instant::now());
            }
            permission.enabled.store(
                engine.settings.polling_controls
                    && permission.desired_enabled.load(Ordering::Acquire)
                    && !permission.suspended.load(Ordering::Acquire),
                Ordering::Release,
            );
            // A stale acknowledgment cannot authorize work with a newer epoch.
            permission.acknowledged.store(epoch, Ordering::Release);
        }
        let requested_suspend = permission.suspended.load(Ordering::Acquire);
        if requested_suspend != suspended {
            suspended = requested_suspend;
            control_cancel.store(true, Ordering::Relaxed);
            hid.invalidate();
            usage_tracker.invalidate(
                hid.generation(),
                engine.settings.polling_controls && !suspended,
            );
            let _ = events.send(Event::PollingInvalidated(hid.generation()));
            if suspended {
                engine.suspend();
            } else {
                engine.resume();
                control_cancel = Arc::new(AtomicBool::new(!engine.settings.polling_controls));
                for id in providers.keys() {
                    due.insert(id, Instant::now());
                }
            }
        }
        usage_tracker.synchronize(
            hid.generation(),
            engine.settings.polling_controls && !suspended,
        );
        usage_tracker.synchronize_clock(clock.unix());
        let quiet = engine.settings.quiet_fullscreen && hb_windows::system::gaming();
        leave_quiet_mode(
            was_quiet,
            quiet,
            providers.keys().copied(),
            &mut due,
            Instant::now(),
        );
        was_quiet = quiet;
        let visible = configuration.visible.load(Ordering::Acquire) && !suspended && !stop;
        if visible && !configuration_visible {
            configuration_due = Instant::now();
            configuration_dirty = true;
        }
        configuration_visible = visible;
        if configuration_generation != hid.generation() {
            configuration_generation = hid.generation();
            configuration_valid = false;
            for device in &mut configuration_devices {
                device.connection = Connection::Stale;
            }
            configuration_due = Instant::now();
            configuration_dirty = true;
        }
        if visible && !configuration_inflight && Instant::now() >= configuration_due {
            if simulate {
                let devices = if simulate_keyboards {
                    simulated_keyboards()
                } else {
                    Vec::new()
                };
                configuration_dirty |= configuration_devices != devices
                    || !configuration_valid
                    || configuration_failure.is_some();
                configuration_devices = devices;
                configuration_valid = true;
                configuration_failure = None;
                configuration_due = Instant::now() + Duration::from_secs(30);
            } else if jobs
                .try_send(WorkerJob::Configuration(
                    configuration.clone(),
                    configuration.epoch.load(Ordering::Acquire),
                    hid.generation(),
                ))
                .is_ok()
            {
                configuration_inflight = true;
                inflight += 1;
                configuration_due = Instant::now() + Duration::from_secs(30);
            }
        }
        if visible
            && configuration_dirty
            && !events.tx.is_full()
            && events
                .try_send(Event::ConfigurationInventory {
                    generation: configuration_generation,
                    devices: configuration_devices.clone(),
                    failure: configuration_failure.clone(),
                })
                .is_ok()
        {
            configuration_dirty = false;
        }
        let ready = providers
            .keys()
            .filter(|id| {
                poll_is_due(
                    &engine.settings,
                    id,
                    due.get(*id).copied(),
                    Instant::now(),
                    stop || suspended,
                )
            })
            .copied()
            .collect::<Vec<_>>();
        if simulate && !ready.is_empty() {
            let mut r = Reading::new(
                "simulated:mouse",
                "Simulated mouse",
                "simulation",
                clock.unix(),
            );
            r.level = Some(73);
            r.charging = Some(true);
            r.kind = "mouse".into();
            engine.apply(
                "simulation",
                Ok(vec![r]),
                clock.monotonic().as_secs_f64(),
                false,
            );
            for t in due.values_mut() {
                *t = Instant::now() + effective_interval(engine.settings.interval, quiet);
            }
            usage_tracker.synchronize(
                hid.generation(),
                engine.settings.polling_controls && !suspended,
            );
            let _ = storage.send(Storage::UsageSample(
                usage_tracker.samples(engine.readings()),
            ));
            let _ = events.try_send(Event::Snapshot(engine.snapshot(clock.unix())));
        } else {
            for id in ready {
                let target = if ["bluetooth", "xinput"].contains(&id) {
                    &winrt
                } else {
                    &jobs
                };
                let mut provider = providers.remove(id).unwrap();
                if invalidated.remove(id) {
                    provider.invalidate();
                }
                let job = Job::new(provider, &engine.settings);
                match target.try_send(WorkerJob::Battery(job)) {
                    Ok(()) => {
                        inflight += 1;
                        due.remove(id);
                    }
                    Err(e) => {
                        if let WorkerJob::Battery(j) = e.into_inner() {
                            providers.insert(id, j.provider);
                        }
                    }
                }
            }
        }
        if stop && inflight == 0 {
            break;
        }
        let wait = due
            .iter()
            .filter(|(id, _)| {
                !suspended && !stop && engine.settings.enabled(id) && providers.contains_key(*id)
            })
            .map(|(_, t)| t.saturating_duration_since(Instant::now()))
            .min()
            .unwrap_or(Duration::from_secs(5))
            .min(Duration::from_secs(5))
            .max(Duration::from_millis(20));
        let work = select! {recv(commands)->m=>Work::Command(m),recv(event_rx)->_=>Work::ConnectionEvent,recv(result_rx)->r=>Work::Completed(r),default(wait)=>Work::Idle};
        match work {
            Work::Command(Ok(Command::Quit)) | Work::Command(Err(_)) => {
                stop = true;
                control_cancel.store(true, Ordering::Relaxed);
                cancel.store(true, Ordering::Relaxed);
            }
            Work::Command(Ok(Command::EpochSuspend(epoch)))
                if epoch == permission.epoch.load(Ordering::Acquire) =>
            {
                suspended = true;
                control_cancel.store(true, Ordering::Relaxed);
                hid.invalidate();
                usage_tracker.invalidate(
                    hid.generation(),
                    engine.settings.polling_controls && !suspended,
                );
                let _ = events.send(Event::PollingInvalidated(hid.generation()));
                engine.suspend();
            }
            Work::Command(Ok(Command::EpochResume(epoch)))
                if epoch == permission.epoch.load(Ordering::Acquire) =>
            {
                suspended = false;
                control_cancel = Arc::new(AtomicBool::new(!engine.settings.polling_controls));
                engine.resume();
                permission.enabled.store(
                    engine.settings.polling_controls
                        && permission.desired_enabled.load(Ordering::Acquire)
                        && !permission.suspended.load(Ordering::Acquire),
                    Ordering::Release,
                );
                permission.acknowledged.store(epoch, Ordering::Release);
                hid.invalidate();
                usage_tracker.invalidate(
                    hid.generation(),
                    engine.settings.polling_controls && !suspended,
                );
                let _ = events.send(Event::PollingInvalidated(hid.generation()));
                for id in hb_providers::provider::FAMILIES
                    .iter()
                    .map(|p| p.0)
                    .chain(["bluetooth", "xinput"])
                {
                    invalidated.insert(id);
                    due.insert(id, Instant::now());
                }
            }
            Work::Command(Ok(Command::NotificationFailed(n))) => {
                if failed_delivery.len() < 512 {
                    failed_delivery.insert(
                        (n.key.clone(), n.kind),
                        (Instant::now() + Duration::from_secs(30), n),
                    );
                }
            }
            Work::Command(Ok(Command::Refresh)) => {
                hid.invalidate();
                usage_tracker.invalidate(
                    hid.generation(),
                    engine.settings.polling_controls && !suspended,
                );
                let _ = events.send(Event::PollingInvalidated(hid.generation()));
                for id in hb_providers::provider::FAMILIES
                    .iter()
                    .map(|p| p.0)
                    .chain(["bluetooth", "xinput"])
                {
                    invalidated.insert(id);
                    due.insert(id, Instant::now());
                }
            }
            Work::Command(Ok(Command::ConfigurationVisible(_))) => {}
            Work::Command(Ok(Command::ConfigurationRefresh)) => {
                configuration_due = Instant::now();
            }
            Work::Command(Ok(Command::History {
                key,
                since,
                until,
                width,
                request,
            })) => {
                let _ = storage.send(Storage::History(key, since, until, width, request));
            }
            Work::Command(Ok(Command::UsageHistory {
                key,
                seconds,
                until,
                width,
                request,
            })) => {
                let _ = storage.send(Storage::UsageHistory(key, seconds, until, width, request));
            }
            Work::Command(Ok(Command::Insights {
                key,
                until,
                request,
            })) => {
                let _ = storage.send(Storage::Insights(key, until, request));
            }
            Work::ConnectionEvent => {
                hid.invalidate();
                usage_tracker.invalidate(
                    hid.generation(),
                    engine.settings.polling_controls && !suspended,
                );
                let _ = events.send(Event::PollingInvalidated(hid.generation()));
                for id in ["bluetooth", "xinput"] {
                    invalidated.insert(id);
                    due.insert(id, Instant::now() + Duration::from_secs(2));
                }
            }
            Work::Completed(Ok(WorkerCompleted::Battery(r))) => {
                inflight = inflight.saturating_sub(1);
                let id = r.provider.id();
                diagnostics_dirty |=
                    update_diagnostics(&mut diagnostics, id.to_string(), r.provider.diagnostics());
                for n in engine.apply(id, r.result, clock.monotonic().as_secs_f64(), quiet) {
                    let _ = events.send(Event::Alert(n));
                }
                let delay = provider_delay(
                    engine.settings.interval,
                    quiet,
                    r.provider.next_poll_delay(),
                );
                due.entry(id).or_insert_with(|| Instant::now() + delay);
                providers.insert(id, r.provider);
                usage_tracker.synchronize(
                    hid.generation(),
                    engine.settings.polling_controls && !suspended,
                );
                let _ = storage.send(Storage::UsageSample(
                    usage_tracker.samples(engine.readings()),
                ));
            }
            Work::Completed(Ok(WorkerCompleted::Configuration(epoch, generation, result))) => {
                inflight = inflight.saturating_sub(1);
                configuration_inflight = false;
                if configuration.active(epoch)
                    && generation == hid.generation()
                    && !suspended
                    && !stop
                {
                    configuration_generation = generation;
                    match result {
                        Ok(devices) => {
                            configuration_dirty |= configuration_devices != devices
                                || !configuration_valid
                                || configuration_failure.is_some();
                            configuration_devices = devices;
                            configuration_valid = true;
                            configuration_failure = None;
                        }
                        Err(error) => {
                            configuration_dirty = true;
                            configuration_valid = false;
                            for device in &mut configuration_devices {
                                device.connection = Connection::Stale;
                            }
                            configuration_failure = Some(error.to_string());
                        }
                    }
                } else {
                    configuration_due = Instant::now();
                }
            }
            Work::Command(Ok(Command::Polling(request, epoch, configuration_epoch))) => {
                let allowed = epoch == permission.epoch.load(Ordering::Acquire)
                    && permission.enabled.load(Ordering::Acquire)
                    && permission.desired_enabled.load(Ordering::Acquire)
                    && permission.acknowledged.load(Ordering::Acquire) == epoch
                    && engine.settings.polling_controls
                    && !suspended
                    && !stop
                    && (request.target.device.kind != "keyboard"
                        || configuration_epoch.is_some_and(|epoch| configuration.active(epoch)))
                    && (engine
                        .readings()
                        .iter()
                        .any(|r| request.target.device.matches_reading(r) && r.online())
                        || (configuration_valid
                            && configuration_generation == hid.generation()
                            && configuration_devices
                                .iter()
                                .any(|d| configuration_matches(&request.target.device, d))));
                if !allowed {
                    let _ = events.send(Event::Polling(Box::new(ControlOutcome::failed(
                        &request,
                        "Enable polling controls and select an online device before configuring it",
                    ))));
                } else if simulate {
                    let outcome = simulate_control(
                        &request,
                        simulated_rates
                            .entry(request.target.device.key.clone())
                            .or_insert_with(|| PollingRate::try_from(1000).unwrap()),
                        hid.generation(),
                        clock.unix(),
                    );
                    if changed_configuration(&outcome, &mut observed_rates)
                        && engine.readings().iter().any(|r| r.key == outcome.key)
                    {
                        engine.reset_estimate(&outcome.key);
                        let _ = storage.send(Storage::State(engine.estimator.clone()));
                    }
                    usage_tracker.synchronize(
                        hid.generation(),
                        engine.settings.polling_controls && !suspended,
                    );
                    usage_tracker.observe(&outcome);
                    let _ = events.send(Event::Polling(Box::new(outcome)));
                } else {
                    match jobs.try_send(WorkerJob::Polling(
                        Box::new(request.clone()),
                        control_cancel.clone(),
                        epoch,
                        configuration_epoch,
                    )) {
                        Ok(()) => inflight += 1,
                        Err(_) => {
                            let _ = events.send(Event::Polling(Box::new(ControlOutcome::failed(
                                &request,
                                "Device workers are busy; retry Refresh or Apply",
                            ))));
                        }
                    }
                }
            }
            Work::Completed(Ok(WorkerCompleted::Polling(mut outcome))) => {
                inflight = inflight.saturating_sub(1);
                if outcome
                    .observation
                    .as_ref()
                    .is_some_and(|o| o.target.generation != hid.generation())
                {
                    outcome.observation = None;
                    outcome.failure = Some(
                        "Connection changed during configuration; refresh before retrying".into(),
                    );
                }
                if changed_configuration(&outcome, &mut observed_rates)
                    && engine.readings().iter().any(|r| r.key == outcome.key)
                {
                    engine.reset_estimate(&outcome.key);
                    let _ = storage.send(Storage::State(engine.estimator.clone()));
                }
                diagnostics_dirty |= update_diagnostics(
                    &mut diagnostics,
                    "polling_controls".into(),
                    vec![format!(
                        "request {}: {}",
                        outcome.request,
                        outcome
                            .failure
                            .as_deref()
                            .unwrap_or("hardware readback verified")
                    )],
                );
                usage_tracker.synchronize(
                    hid.generation(),
                    engine.settings.polling_controls && !suspended,
                );
                usage_tracker.observe(&outcome);
                let _ = events.send(Event::Polling(outcome));
            }
            Work::Command(Ok(
                Command::SettingsChanged
                | Command::EpochSuspend(_)
                | Command::EpochResume(_)
                | Command::Settings(_)
                | Command::Suspend
                | Command::Resume,
            )) => {}
            Work::Completed(Err(_)) | Work::Idle => {}
        }
        let snapshot = engine.snapshot(clock.unix());
        if engine.settings.status_file {
            let _ = events.try_send(Event::Snapshot(snapshot.clone()));
            let _ = storage.send(Storage::Status(snapshot, true));
        } else {
            let _ = events.try_send(Event::Snapshot(snapshot));
        }
        publish_diagnostics(&events, &diagnostics, &mut diagnostics_dirty);
        let retry = failed_delivery
            .iter()
            .filter(|(_, v)| v.0 <= Instant::now())
            .map(|(k, _)| k.clone())
            .collect::<Vec<_>>();
        for key in retry {
            if let Some((_, n)) = failed_delivery.remove(&key) {
                engine.notification_failed(n);
            }
        }
        if !suspended && (!engine.settings.quiet_fullscreen || !hb_windows::system::gaming()) {
            for n in engine.flush_held() {
                let _ = events.send(Event::Alert(n));
            }
        }
    }
    drop(jobs);
    drop(winrt);
    for w in workers {
        let _ = w.join();
    }
    let _ = storage.send(Storage::State(engine.estimator.clone()));
    if engine.settings.status_file {
        let _ = storage.send(Storage::Status(engine.snapshot(clock.unix()), false));
    }
    let _ = storage.send(Storage::Quit);
    let _ = store_thread.join();
}

#[cfg(test)]
mod tests {
    #[derive(Default)]
    struct PassiveHid {
        enumerations: AtomicU64,
        generation: AtomicU64,
        close_after_enumeration: Option<Arc<ConfigurationWatch>>,
    }
    impl HidTransport for PassiveHid {
        fn generation(&self) -> u64 {
            self.generation.load(Ordering::Relaxed)
        }
        fn enumerate(&self, vendor: u16) -> Result<Vec<HidInfo>, ProviderError> {
            self.enumerations.fetch_add(1, Ordering::Relaxed);
            if let Some(watch) = &self.close_after_enumeration {
                watch.set_visible(false);
            }
            Ok(if vendor == 0x1532 {
                vec![HidInfo {
                    vendor_id: vendor,
                    product_id: 0x026c,
                    interface: 3,
                    feature_length: Some(91),
                    path: "hid#test-keyboard".into(),
                    serial: "TEST-KEYBOARD".into(),
                    container: Some("test-container".into()),
                    ..Default::default()
                }]
            } else {
                Vec::new()
            })
        }
        fn open(&self, _: &HidInfo) -> Result<Box<dyn HidSession>, ProviderError> {
            panic!("passive discovery cannot open")
        }
    }
    #[test]
    fn configuration_discovery_stops_when_closed_and_rejects_old_visibility_and_connection_epochs()
    {
        let hid = PassiveHid::default();
        let watch = ConfigurationWatch::default();
        let clock = SystemClock::default();
        let cancel = AtomicBool::new(false);
        assert!(discover_configuration(&hid, &clock, &cancel, &watch, 0, 0).is_err());
        assert_eq!(hid.enumerations.load(Ordering::Relaxed), 0);
        watch.set_visible(true);
        let epoch = watch.epoch.load(Ordering::Acquire);
        assert_eq!(
            discover_configuration(&hid, &clock, &cancel, &watch, epoch, 0)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(hid.enumerations.load(Ordering::Relaxed), 2);
        watch.set_visible(false);
        watch.set_visible(true);
        assert!(discover_configuration(&hid, &clock, &cancel, &watch, epoch, 0).is_err());
        assert_eq!(hid.enumerations.load(Ordering::Relaxed), 2);
        hid.generation.store(1, Ordering::Relaxed);
        assert!(
            discover_configuration(
                &hid,
                &clock,
                &cancel,
                &watch,
                watch.epoch.load(Ordering::Acquire),
                0
            )
            .is_err()
        );
        assert_eq!(hid.enumerations.load(Ordering::Relaxed), 2);
        let watch = Arc::new(ConfigurationWatch::default());
        watch.set_visible(true);
        let hid = PassiveHid {
            close_after_enumeration: Some(watch.clone()),
            ..Default::default()
        };
        assert!(discover_configuration(&hid, &clock, &cancel, &watch, 1, 0).is_err());
        assert_eq!(hid.enumerations.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn keyboard_requests_do_not_access_hid_after_dashboard_closes_or_reopens() {
        let hid = PassiveHid::default();
        let watch = Arc::new(ConfigurationWatch::default());
        watch.set_visible(true);
        let epoch = watch.epoch.load(Ordering::Acquire);
        watch.set_visible(false);
        watch.set_visible(true);
        let clock = SystemClock::default();
        let cancel = Arc::new(AtomicBool::new(false));
        for action in [
            ControlAction::Read,
            ControlAction::Apply(PollingRate::try_from(250).unwrap()),
        ] {
            let request = ControlRequest {
                request: 1,
                target: ControlTarget {
                    device: simulated_keyboards().remove(0),
                    generation: 0,
                },
                action,
            };
            let outcome = execute_control_inner(
                &request,
                &hid,
                &clock,
                &cancel,
                &cancel,
                false,
                ControlGuards {
                    permission: None,
                    configuration: Some((watch.clone(), epoch)),
                },
            );
            assert!(outcome.failure.unwrap().contains("no longer visible"));
            assert!(!outcome.may_have_changed);
        }
        assert_eq!(hid.enumerations.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn queued_keyboard_commands_keep_submission_epoch_even_when_close_notifications_cannot_enqueue()
    {
        for action in [
            ControlAction::Read,
            ControlAction::Apply(PollingRate::try_from(250).unwrap()),
        ] {
            let (commands, rx) = bounded(1);
            let (tx, events) = bounded(4);
            let configuration = Arc::new(ConfigurationWatch::default());
            configuration.set_visible(true);
            let runtime = Runtime {
                commands,
                events,
                thread: None,
                permission: Arc::new(ControlPermission::new(true)),
                sink: Events {
                    tx,
                    window: Arc::new(AtomicUsize::new(0)),
                },
                cancel: Arc::new(AtomicBool::new(false)),
                window: Arc::new(AtomicUsize::new(0)),
                configuration,
            };
            runtime
                .submit_control(ControlRequest {
                    request: 1,
                    target: ControlTarget {
                        device: simulated_keyboards().remove(0),
                        generation: 0,
                    },
                    action,
                })
                .unwrap();
            runtime.send(Command::ConfigurationVisible(false));
            runtime.send(Command::ConfigurationVisible(true));
            let Command::Polling(_, _, Some(submitted_epoch)) = rx.recv().unwrap() else {
                panic!("keyboard command missing submission epoch")
            };
            assert_eq!(submitted_epoch, 1);
            assert!(!runtime.configuration.active(submitted_epoch));
            assert!(runtime.configuration.visible.load(Ordering::Acquire));
        }
    }
    #[test]
    fn batteryless_configuration_never_creates_battery_samples_and_has_independent_rates() {
        let mut devices = simulated_keyboards();
        let keyboard = devices.remove(0);
        let unavailable = devices.remove(0);
        let mut rate = PollingRate::try_from(1000).unwrap();
        let request = ControlRequest {
            request: 1,
            target: ControlTarget {
                device: keyboard.clone(),
                generation: 0,
            },
            action: ControlAction::Apply(PollingRate::try_from(250).unwrap()),
        };
        let outcome = simulate_control(&request, &mut rate, 0, 10);
        assert_eq!(rate.hz(), 250);
        let mut tracker = UsageTracker::new(0, true);
        tracker.observe(&outcome);
        assert!(tracker.samples(Vec::new()).is_empty());
        assert!(tracker.rates.is_empty());
        let mut changed = keyboard.clone();
        changed.container = Some("different".into());
        assert!(!configuration_matches(&keyboard, &changed));
        changed = keyboard.clone();
        changed.capability = PollingCapability::Unavailable("refused".into());
        assert!(!configuration_matches(&keyboard, &changed));
        let mut engine = Engine::new(
            Settings {
                disabled_providers: BTreeSet::from(["razer".into()]),
                ..Default::default()
            },
            Estimator::default(),
        );
        assert!(engine.readings().is_empty());
        assert!(configuration_matches(&keyboard, &keyboard));
        assert!(engine.snapshot(10).devices.is_empty());
        assert!(engine.flush_held().is_empty());
        let request = ControlRequest {
            target: ControlTarget {
                device: unavailable,
                generation: 0,
            },
            ..request
        };
        assert!(
            simulate_control(&request, &mut rate, 0, 11)
                .failure
                .is_some()
        );
        assert_eq!(rate.hz(), 250);
    }
    #[test]
    fn diagnostics_publish_only_changes_and_retry_after_backpressure() {
        let (tx, rx) = bounded(1);
        let events = Events {
            tx,
            window: Arc::new(AtomicUsize::new(0)),
        };
        let mut diagnostics = BTreeMap::new();
        let mut dirty = update_diagnostics(&mut diagnostics, "test".into(), vec!["ready".into()]);
        publish_diagnostics(&events, &diagnostics, &mut dirty);
        assert!(!dirty);
        assert!(!update_diagnostics(
            &mut diagnostics,
            "test".into(),
            vec!["ready".into()]
        ));
        publish_diagnostics(&events, &diagnostics, &mut dirty);
        assert_eq!(rx.len(), 1);
        dirty |= update_diagnostics(&mut diagnostics, "test".into(), vec!["changed".into()]);
        publish_diagnostics(&events, &diagnostics, &mut dirty);
        assert!(dirty);
        let Event::Diagnostics(first) = rx.recv().unwrap() else {
            panic!("diagnostics expected")
        };
        assert_eq!(first["test"], ["ready"]);
        publish_diagnostics(&events, &diagnostics, &mut dirty);
        assert!(!dirty);
        let Event::Diagnostics(second) = rx.recv().unwrap() else {
            panic!("diagnostics expected")
        };
        assert_eq!(second["test"], ["changed"]);
        publish_diagnostics(&events, &diagnostics, &mut dirty);
        assert!(rx.is_empty());
    }
    use super::*;
    #[test]
    fn usage_learning_requires_fresh_readback_and_matching_connection() {
        let mut tracker = UsageTracker::new(2, true);
        let request = control_request(ControlAction::Read, 2);
        let mut reading = control_reading();
        reading.level = Some(80);
        reading.charging = Some(false);
        reading.timestamp = 19;
        assert!(
            tracker.samples(vec![reading.clone()])[0]
                .polling_rate
                .is_none()
        );
        let mut rate = PollingRate::try_from(1000).unwrap();
        let outcome = simulate_control(&request, &mut rate, 2, 20);
        tracker.observe(&outcome);
        // Never attach a new confirmation to an older cached observation.
        assert!(
            tracker.samples(vec![reading.clone()])[0]
                .polling_rate
                .is_none()
        );
        reading.timestamp = 21;
        assert_eq!(
            tracker.samples(vec![reading.clone()])[0].polling_rate,
            Some(rate)
        );
        let previous_session = tracker.samples(vec![reading.clone()])[0].session;
        tracker.synchronize(3, true);
        let disconnected = tracker.samples(vec![reading.clone()]);
        assert!(disconnected[0].polling_rate.is_none());
        assert_ne!(disconnected[0].session, previous_session);
        tracker.observe(&outcome);
        assert!(
            tracker.samples(vec![reading.clone()])[0]
                .polling_rate
                .is_none()
        );
        tracker.synchronize(2, true);
        tracker.observe(&outcome);
        reading.via = "other transport".into();
        assert!(tracker.samples(vec![reading])[0].polling_rate.is_none());
    }
    #[test]
    fn failed_changes_disable_and_backward_clock_revoke_usage_evidence() {
        let request = control_request(ControlAction::Read, 2);
        let mut reading = control_reading();
        reading.timestamp = 30;
        let mut rate = PollingRate::try_from(1000).unwrap();
        let confirmed = simulate_control(&request, &mut rate, 2, 20);
        let mut tracker = UsageTracker::new(2, true);
        tracker.observe(&confirmed);
        let mut failed = confirmed.clone();
        failed.failure = Some("partial write".into());
        tracker.observe(&failed);
        assert!(
            tracker.samples(vec![reading.clone()])[0]
                .polling_rate
                .is_none()
        );
        tracker.observe(&confirmed);
        tracker.synchronize(2, false);
        tracker.observe(&confirmed);
        assert!(
            tracker.samples(vec![reading.clone()])[0]
                .polling_rate
                .is_none()
        );
        tracker.synchronize(2, true);
        tracker.observe(&confirmed);
        tracker.synchronize_clock(30);
        tracker.synchronize_clock(29);
        assert!(tracker.samples(vec![reading])[0].polling_rate.is_none());
    }
    #[test]
    fn insights_worker_flushes_rate_metadata_before_query() {
        let directory = tempfile::tempdir().unwrap();
        let (tx, rx) = bounded(16);
        let (boot_tx, boot_rx) = bounded(1);
        let (event_tx, events) = bounded(16);
        let sink = Events {
            tx: event_tx,
            window: Arc::new(AtomicUsize::new(0)),
        };
        let folder = directory.path().to_path_buf();
        let worker = thread::spawn(move || crate::storage_worker::run(folder, rx, boot_tx, sink));
        let _ = boot_rx.recv().unwrap();
        let rate = PollingRate::try_from(1000).unwrap();
        for i in 0..=3 {
            let mut reading = Reading::new("synthetic", "Test mouse", "test", 100 + i * 600);
            reading.level = Some(80 - i as u8);
            reading.charging = Some(false);
            tx.send(Storage::UsageSample(vec![UsageObservation {
                reading,
                polling_rate: Some(rate),
                session: Some(1),
            }]))
            .unwrap();
        }
        tx.send(Storage::Insights("synthetic".into(), 1900, 12))
            .unwrap();
        let Event::Insights(12, result) = events.recv_timeout(Duration::from_secs(5)).unwrap()
        else {
            panic!("missing insights")
        };
        let summary = result.unwrap();
        assert_eq!(summary.rates[0].awake_seconds, 1800);
        assert_eq!(summary.rates[0].consumed_percent, 3);
        assert!(summary.rates[0].projected_full_charge_hours.is_some());
        tx.send(Storage::Quit).unwrap();
        worker.join().unwrap();
        let reopened = hb_storage::Store::read_only(&directory.path().join("history.db")).unwrap();
        assert_eq!(
            reopened.query_insights("synthetic", 1900).unwrap().rates[0].consumed_percent,
            3
        );
    }
    #[test]
    fn connection_events_cover_hid_bluetooth_and_le_case_insensitively() {
        for id in [
            "\\\\?\\hid#vid_1532&pid_00bf",
            "BTHENUM\\DEV_0123456789AB",
            "bthledevice#battery",
            "Bluetooth#battery",
        ] {
            assert!(relevant_device_event(id), "{id}");
        }
        for id in ["", "USB#printer", "SWD#audioendpoint"] {
            assert!(!relevant_device_event(id));
        }
    }
    #[test]
    fn gaming_cadence_never_speeds_up_a_slow_setting() {
        assert_eq!(effective_interval(60, false), Duration::from_secs(60));
        assert_eq!(effective_interval(60, true), Duration::from_secs(300));
        assert_eq!(effective_interval(600, true), Duration::from_secs(600));
        assert_eq!(
            provider_delay(60, true, Some(Duration::from_secs(3))),
            Duration::from_secs(300)
        );
        assert_eq!(
            provider_delay(600, true, Some(Duration::from_secs(3))),
            Duration::from_secs(600)
        );
        assert_eq!(
            provider_delay(60, false, Some(Duration::from_secs(3))),
            Duration::from_secs(3)
        );
    }
    #[test]
    fn leaving_gaming_mode_refreshes_immediately_but_entering_does_not() {
        let now = Instant::now();
        let future = now + Duration::from_secs(300);
        let mut due = BTreeMap::from([("razer", future), ("bluetooth", future)]);
        leave_quiet_mode(
            false,
            true,
            ["razer", "bluetooth"].into_iter(),
            &mut due,
            now,
        );
        assert!(due.values().all(|v| *v == future));
        leave_quiet_mode(
            true,
            false,
            ["razer", "bluetooth"].into_iter(),
            &mut due,
            now,
        );
        assert!(due.values().all(|v| *v == now));
    }
    #[test]
    fn disabled_bluetooth_providers_and_suspended_jobs_are_not_dispatched() {
        let now = Instant::now();
        let mut s = Settings::default();
        assert!(poll_is_due(&s, "razer", Some(now), now, false));
        assert!(!poll_is_due(&s, "razer", Some(now), now, true));
        assert!(!poll_is_due(&s, "razer", None, now, false));
        assert!(!poll_is_due(
            &s,
            "razer",
            Some(now + Duration::from_secs(1)),
            now,
            false,
        ));
        s.bluetooth = false;
        s.disabled_providers.insert("razer".into());
        assert!(!poll_is_due(&s, "razer", Some(now), now, false));
        assert!(!poll_is_due(&s, "bluetooth", Some(now), now, false));
        assert!(poll_is_due(&s, "xinput", Some(now), now, false));
        s.playstation_full_mode = true;
        s.disabled_providers.insert("playstation".into());
        assert!(!poll_is_due(&s, "playstation", Some(now), now, false));
    }
    #[test]
    fn full_mode_setting_reaches_provider_context_and_cancelled_jobs_do_not_poll() {
        struct Provider(Arc<Mutex<Vec<bool>>>);
        impl BatteryProvider for Provider {
            fn diagnostics(&self) -> Vec<String> {
                vec![]
            }
            fn id(&self) -> &'static str {
                "playstation"
            }
            fn poll(&mut self, _: &dyn HidTransport, c: &PollContext<'_>) -> PollResult {
                self.0.lock().unwrap().push(c.playstation_full_mode);
                Ok(vec![])
            }
        }
        struct NoHid;
        impl HidTransport for NoHid {
            fn enumerate(&self, _: u16) -> Result<Vec<HidInfo>, ProviderError> {
                panic!("unexpected HID")
            }
            fn open(&self, _: &HidInfo) -> Result<Box<dyn HidSession>, ProviderError> {
                panic!("unexpected HID")
            }
        }
        let observed = Arc::new(Mutex::new(vec![]));
        let mut s = Settings::default();
        let cancel = AtomicBool::new(false);
        let clock = SystemClock::default();
        for full in [false, true] {
            s.playstation_full_mode = full;
            let job = Job::new(Box::new(Provider(observed.clone())), &s);
            assert!(execute_job(job, &NoHid, &clock, &cancel).result.is_ok());
        }
        cancel.store(true, Ordering::Relaxed);
        let job = Job::new(Box::new(Provider(observed.clone())), &s);
        assert!(execute_job(job, &NoHid, &clock, &cancel).result.is_err());
        assert_eq!(*observed.lock().unwrap(), [false, true]);
    }
    fn control_reading() -> Reading {
        let mut reading = Reading::new("simulated:mouse", "Mouse", "simulation", 10);
        reading.kind = "mouse".into();
        reading
    }
    fn control_request(action: ControlAction, generation: u64) -> ControlRequest {
        let reading = control_reading();
        ControlRequest {
            request: 1,
            target: ControlTarget {
                device: ConfigurationDevice::from_reading(&reading),
                generation,
            },
            action,
        }
    }
    #[test]
    fn explicit_simulation_read_apply_restore_and_stale_epoch() {
        let mut current = PollingRate::try_from(1000).unwrap();
        let read = simulate_control(
            &control_request(ControlAction::Read, 0),
            &mut current,
            2,
            10,
        );
        assert_eq!(read.observation.unwrap().rate.unwrap().hz(), 1000);
        assert!(!read.may_have_changed);
        let target = control_request(
            ControlAction::Apply(PollingRate::try_from(8000).unwrap()),
            2,
        );
        let applied = simulate_control(&target, &mut current, 2, 11);
        assert!(applied.confirmed_change());
        assert_eq!(applied.previous.unwrap().hz(), 1000);
        assert_eq!(current.hz(), 8000);
        assert!(
            simulate_control(&target, &mut current, 3, 12)
                .failure
                .is_some()
        );
        let restored = simulate_control(
            &control_request(ControlAction::Apply(applied.previous.unwrap()), 2),
            &mut current,
            2,
            13,
        );
        assert!(restored.confirmed_change());
        assert_eq!(current.hz(), 1000);
    }
    #[test]
    fn gaming_or_cancelled_controls_never_open_hid() {
        struct NeverHid;
        impl HidTransport for NeverHid {
            fn enumerate(&self, _: u16) -> Result<Vec<HidInfo>, ProviderError> {
                panic!("unexpected access")
            }
            fn open(&self, _: &HidInfo) -> Result<Box<dyn HidSession>, ProviderError> {
                panic!("unexpected access")
            }
        }
        let clock = SystemClock::default();
        let shutdown = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut request = control_request(
            ControlAction::Apply(PollingRate::try_from(8000).unwrap()),
            0,
        );
        for provider in hb_providers::controls::POLLING_PROVIDERS {
            request.target.device.source = (*provider).into();
            assert!(
                execute_control(
                    &request, &NeverHid, &clock, &shutdown, &cancelled, true, None
                )
                .failure
                .unwrap()
                .contains("Close the game")
            );
        }
        cancelled.store(true, Ordering::Relaxed);
        assert!(
            execute_control(
                &request, &NeverHid, &clock, &shutdown, &cancelled, false, None
            )
            .failure
            .is_some()
        );
        cancelled.store(false, Ordering::Relaxed);
        shutdown.store(true, Ordering::Relaxed);
        assert!(
            execute_control(
                &request, &NeverHid, &clock, &shutdown, &cancelled, false, None
            )
            .failure
            .is_some()
        );
    }
    #[test]
    fn full_command_queue_reports_rejection_without_silent_drop() {
        let (commands, _rx) = bounded(1);
        commands.send(Command::Refresh).unwrap();
        let (event_tx, events) = bounded(1);
        let runtime = Runtime {
            configuration: Arc::new(ConfigurationWatch::default()),
            commands,
            events,
            thread: None,
            permission: Arc::new(ControlPermission::new(true)),
            sink: Events {
                tx: event_tx,
                window: Arc::new(AtomicUsize::new(0)),
            },
            cancel: Arc::new(AtomicBool::new(false)),
            window: Arc::new(AtomicUsize::new(0)),
        };
        assert!(
            runtime
                .submit_control(control_request(ControlAction::Read, 0))
                .is_err()
        );
    }
    #[test]
    fn full_queue_cannot_lose_configuration_revocation_or_revive_old_jobs() {
        let (commands, _rx) = bounded(1);
        commands.send(Command::Refresh).unwrap();
        let (event_tx, events) = bounded(4);
        let permission = Arc::new(ControlPermission::new(true));
        let runtime = Runtime {
            configuration: Arc::new(ConfigurationWatch::default()),
            commands,
            events,
            thread: None,
            permission: permission.clone(),
            sink: Events {
                tx: event_tx,
                window: Arc::new(AtomicUsize::new(0)),
            },
            cancel: Arc::new(AtomicBool::new(false)),
            window: Arc::new(AtomicUsize::new(0)),
        };
        runtime.send(Command::Settings(Settings::default()));
        assert!(!permission.enabled.load(Ordering::Acquire));
        assert_eq!(permission.epoch.load(Ordering::Acquire), 1);
        assert!(
            runtime
                .submit_control(control_request(ControlAction::Read, 0))
                .is_err()
        );
        assert!(matches!(runtime.events.try_recv(), Ok(Event::Error(_))));
        runtime.send(Command::Suspend);
        assert!(permission.suspended.load(Ordering::Acquire));
        assert_eq!(permission.epoch.load(Ordering::Acquire), 2);
        // Even a later enabling acknowledgment cannot authorize a job with epoch0.
        permission.enabled.store(true, Ordering::Release);
        struct NeverSession;
        impl HidSession for NeverSession {
            fn write(&mut self, _: &[u8]) -> Result<(), ProviderError> {
                panic!("native write")
            }
            fn read(&mut self, _: usize, _: Duration) -> Result<Vec<u8>, ProviderError> {
                panic!("native read")
            }
            fn send_feature(&mut self, _: &[u8]) -> Result<(), ProviderError> {
                panic!("native feature")
            }
            fn feature(&mut self, _: u8, _: usize) -> Result<Vec<u8>, ProviderError> {
                panic!("native feature")
            }
        }
        let mut session = ControlSession {
            configuration: None,
            inner: Box::new(NeverSession),
            shutdown: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            permission: Some((permission.clone(), 0)),
            block_while_gaming: false,
            notification_state: || false,
        };
        assert!(session.send_feature(&[0]).is_err());
        permission.suspended.store(false, Ordering::Release);
        assert!(session.write(&[0]).is_err());
        permission.desired_enabled.store(true, Ordering::Release);
        permission.acknowledged.store(2, Ordering::Release);
        session.permission = Some((permission, 2));
        session.block_while_gaming = true;
        session.notification_state = || true;
        assert!(
            session
                .send_feature(&[0])
                .unwrap_err()
                .message
                .contains("Close the game")
        );
    }
    #[test]
    fn verified_refresh_after_partial_change_resets_estimate_without_claiming_partial_success() {
        let mut current = PollingRate::try_from(1000).unwrap();
        let mut observed = BTreeMap::new();
        let read = simulate_control(
            &control_request(ControlAction::Read, 0),
            &mut current,
            0,
            10,
        );
        assert!(!changed_configuration(&read, &mut observed));
        let mut partial = simulate_control(
            &control_request(
                ControlAction::Apply(PollingRate::try_from(8000).unwrap()),
                0,
            ),
            &mut current,
            0,
            11,
        );
        partial.failure = Some("second SET acknowledgment failed".into());
        assert!(!changed_configuration(&partial, &mut observed));
        assert_eq!(observed["simulated:mouse"].hz(), 1000);
        let confirmed = simulate_control(
            &control_request(ControlAction::Read, 0),
            &mut current,
            0,
            12,
        );
        assert!(changed_configuration(&confirmed, &mut observed));
        assert!(!changed_configuration(&confirmed, &mut observed));
    }
    #[test]
    fn settings_survive_suspend_resume_and_full_mailboxes_shutdown() {
        // WinRT workers own native windows; serialize process-wide handle audits.
        let _native_guard = crate::ui::NATIVE_TEST_LOCK.lock().unwrap();
        let d = tempfile::tempdir().unwrap();
        let mut runtime = Runtime::start(
            d.path().to_owned(),
            Settings {
                polling_controls: true,
                ..Default::default()
            },
            true,
        )
        .unwrap();
        runtime.send(Command::Settings(Settings {
            polling_controls: false,
            low: 17,
            ..Default::default()
        }));
        runtime.send(Command::Suspend);
        runtime.send(Command::Resume);
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            while runtime.events.try_recv().is_ok() {}
            if d.path().join("config.json").exists() {
                let s = hb_storage::load_settings(&d.path().join("config.json"));
                if s.low == 17 && !s.polling_controls {
                    break;
                }
            }
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            runtime
                .submit_control(control_request(ControlAction::Read, 0))
                .is_err()
        );
        // Artificially fill both queues. Stop must drain events before waiting
        // for a Quit slot; its shared cancellation mailbox ends the owner loop.
        while runtime.sink.try_send(Event::Error(String::new())).is_ok() {}
        while runtime.commands.try_send(Command::Refresh).is_ok() {}
        let started = Instant::now();
        runtime.stop();
        assert!(started.elapsed() < Duration::from_secs(5));
    }
    #[test]
    fn simulation_rejects_unadvertised_rate_without_changing_current() {
        let mut current = PollingRate::try_from(1000).unwrap();
        let result = simulate_control(
            &control_request(ControlAction::Apply(PollingRate::try_from(250).unwrap()), 0),
            &mut current,
            0,
            10,
        );
        assert!(result.failure.is_some());
        assert!(!result.may_have_changed);
        assert_eq!(current.hz(), 1000);
    }
    #[test]
    fn background_simulation_does_not_read_or_apply_saved_polling_selection() {
        // WinRT workers own native windows; serialize process-wide handle audits.
        let _native_guard = crate::ui::NATIVE_TEST_LOCK.lock().unwrap();
        let d = tempfile::tempdir().unwrap();
        let mut settings = Settings {
            polling_controls: true,
            ..Default::default()
        };
        settings
            .devices
            .entry("simulated:mouse".into())
            .or_default()
            .requested_polling_rate = PollingRate::try_from(8000).ok();
        let mut runtime = Runtime::start(d.path().to_owned(), settings, true).unwrap();
        loop {
            match runtime
                .events
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
            {
                Event::Polling(_) => panic!("unsolicited configuration request"),
                Event::Snapshot(s) if !s.devices.is_empty() => break,
                _ => {}
            }
        }
        runtime
            .submit_control(control_request(ControlAction::Read, 0))
            .unwrap();
        loop {
            if let Event::Polling(outcome) = runtime
                .events
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
            {
                assert_eq!(outcome.observation.unwrap().rate.unwrap().hz(), 1000);
                break;
            }
        }
        runtime.stop();
    }
    #[test]
    fn status_stays_absent_when_disabled_and_shutdown_drains_queues() {
        // WinRT workers own native windows; serialize process-wide handle audits.
        let _native_guard = crate::ui::NATIVE_TEST_LOCK.lock().unwrap();
        let d = tempfile::tempdir().unwrap();
        let mut runtime = Runtime::start(d.path().to_owned(), Settings::default(), true).unwrap();
        loop {
            if let Event::Snapshot(snapshot) = runtime
                .events
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                && !snapshot.devices.is_empty()
            {
                break;
            }
        }
        runtime.stop();
        assert!(!d.path().join("status.json").exists());
        let s = hb_storage::Store::read_only(&d.path().join("history.db")).unwrap();
        assert!(
            !s.query("simulated:mouse", 0, i64::MAX / 2, 100)
                .unwrap()
                .is_empty()
        );
    }
    fn storage(folder: PathBuf) -> (Sender<Storage>, Receiver<Event>, JoinHandle<()>) {
        let (tx, rx) = bounded(32);
        let (events, output) = bounded(32);
        let (boot, ready) = bounded(1);
        let sink = Events {
            tx: events,
            window: Arc::new(AtomicUsize::new(0)),
        };
        let worker = thread::spawn(move || crate::storage_worker::run(folder, rx, boot, sink));
        let _ = ready.recv_timeout(Duration::from_secs(5)).unwrap();
        (tx, output, worker)
    }
    #[test]
    fn database_failure_preserves_configuration_and_answers_queries() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("history.db"), b"invalid SQLite database").unwrap();
        let (tx, events, worker) = storage(d.path().to_owned());
        tx.send(Storage::Save(Settings {
            low: 17,
            ..Default::default()
        }))
        .unwrap();
        tx.send(Storage::Status(Snapshot::default(), true)).unwrap();
        tx.send(Storage::History("mouse".into(), 0, 100, 50, 9))
            .unwrap();
        loop {
            if let Event::History(9, result) = events.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                assert!(result.is_err());
                break;
            }
        }
        tx.send(Storage::Quit).unwrap();
        worker.join().unwrap();
        assert_eq!(
            hb_storage::load_settings(&d.path().join("config.json")).low,
            17
        );
        assert!(d.path().join("status.json").exists());
        assert_eq!(
            std::fs::read(d.path().join("history.db")).unwrap(),
            b"invalid SQLite database"
        );
    }
    #[test]
    fn usage_history_flushes_pending_rows_and_preserves_real_timestamps() {
        let d = tempfile::tempdir().unwrap();
        let (tx, events, worker) = storage(d.path().to_owned());
        for (timestamp, level) in [(100, 80), (160, 79), (220, 78)] {
            let mut reading = Reading::new("mouse", "Mouse", "razer", timestamp);
            reading.level = Some(level);
            reading.charging = Some(false);
            tx.send(Storage::unconfirmed(vec![reading])).unwrap();
        }
        tx.send(Storage::UsageHistory("mouse".into(), 3600, 220, 40, 11))
            .unwrap();
        loop {
            if let Event::History(11, result) = events.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                let series = result.unwrap();
                assert_eq!(series.axis, HistoryAxis::Usage);
                assert_eq!(series.until, 120);
                assert_eq!(series.samples.first().unwrap().reading.timestamp, 100);
                assert_eq!(series.samples.last().unwrap().reading.timestamp, 220);
                assert_eq!(series.samples.last().unwrap().position, 120);
                break;
            }
        }
        tx.send(Storage::Quit).unwrap();
        worker.join().unwrap();
    }
    #[test]
    fn queued_history_flushes_and_status_can_be_disabled() {
        let d = tempfile::tempdir().unwrap();
        let (tx, events, worker) = storage(d.path().to_owned());
        let mut r = Reading::new("mouse", "Mouse", "razer", 100);
        r.level = Some(80);
        tx.send(Storage::unconfirmed(vec![r])).unwrap();
        tx.send(Storage::Status(Snapshot::default(), true)).unwrap();
        tx.send(Storage::RemoveStatus).unwrap();
        tx.send(Storage::History("mouse".into(), 0, 100, 50, 10))
            .unwrap();
        loop {
            if let Event::History(10, result) = events.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                assert_eq!(result.unwrap().samples[0].reading.level, Some(80));
                break;
            }
        }
        tx.send(Storage::Quit).unwrap();
        worker.join().unwrap();
        assert!(!d.path().join("status.json").exists());
        let store = hb_storage::Store::read_only(&d.path().join("history.db")).unwrap();
        assert_eq!(store.query("mouse", 0, 100, 50).unwrap().len(), 1);
    }
}
