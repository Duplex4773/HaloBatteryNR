//! Engine and storage ownership. Workers move providers, never share mutable state.
use crossbeam_channel::{Receiver, Sender, bounded, select};
use hb_core::*;
use hb_windows::{BluetoothProvider, ControllerProvider, WindowsHid};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
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
    Quit,
}
pub enum Event {
    Snapshot(Snapshot),
    Alert(Notification),
    History(u64, Result<Vec<Reading>, ProviderError>),
    Error(String),
    Diagnostics(BTreeMap<String, Vec<String>>),
}
pub(super) enum Storage {
    Sample(Vec<Reading>),
    Save(Settings),
    State(Estimator),
    Status(Snapshot, bool),
    RemoveStatus,
    History(String, i64, i64, usize, u64),
    Quit,
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
enum Work {
    Command(Result<Command, crossbeam_channel::RecvError>),
    ConnectionEvent,
    Completed(Result<Completed, crossbeam_channel::RecvError>),
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
pub struct Runtime {
    pub commands: Sender<Command>,
    pub events: Receiver<Event>,
    thread: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
    window: Arc<AtomicUsize>,
}
impl Runtime {
    pub fn start(dir: PathBuf, settings: Settings, simulate: bool) -> Result<Self, ProviderError> {
        let hid = Arc::new(WindowsHid::new()?);
        let cancel = Arc::new(AtomicBool::new(false));
        let (commands, rx) = bounded(32);
        let (events, events_rx) = bounded(64);
        let cancelled = cancel.clone();
        let window = Arc::new(AtomicUsize::new(0));
        let sink = Events {
            tx: events,
            window: window.clone(),
        };
        let thread = thread::Builder::new()
            .name("state".into())
            .stack_size(512 * 1024)
            .spawn(move || run(dir, settings, simulate, hid, cancelled, rx, sink))?;
        Ok(Self {
            commands,
            events: events_rx,
            thread: Some(thread),
            cancel,
            window,
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
        let _ = self.commands.try_send(command);
    }
    pub fn stop(&mut self) {
        if self.thread.is_none() {
            return;
        }
        self.cancel.store(true, Ordering::Relaxed);
        let _ = self.commands.send(Command::Quit);
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
    jobs: Receiver<Job>,
    results: Sender<Completed>,
    hid: Arc<WindowsHid>,
    cancel: Arc<AtomicBool>,
    winrt: bool,
    commands: Sender<Command>,
    gates: Arc<BTreeMap<u16, Mutex<()>>>,
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
                let vendors = hb_providers::catalog::DEVICES
                    .iter()
                    .filter(|d| d.provider == job.provider.id())
                    .map(|d| d.vid)
                    .collect::<BTreeSet<_>>();
                let _guards = vendors
                    .iter()
                    .filter_map(|v| gates.get(v))
                    .map(|m| m.lock().unwrap_or_else(|p| p.into_inner()))
                    .collect::<Vec<_>>();
                if results
                    .send(execute_job(job, &*hid, &clock, &cancel))
                    .is_err()
                {
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
    cancel: Arc<AtomicBool>,
    commands: Receiver<Command>,
    events: Events,
) {
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
    let gates = Arc::new(
        hb_providers::catalog::DEVICES
            .iter()
            .map(|d| d.vid)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|v| (v, Mutex::new(())))
            .collect::<BTreeMap<_, _>>(),
    );
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
    let mut stop = false;
    let mut suspended = false;
    let mut was_quiet = false;
    let mut failed_delivery: BTreeMap<(String, NotificationKind), (Instant, Notification)> =
        BTreeMap::new();
    loop {
        let quiet = engine.settings.quiet_fullscreen && hb_windows::system::gaming();
        leave_quiet_mode(
            was_quiet,
            quiet,
            providers.keys().copied(),
            &mut due,
            Instant::now(),
        );
        was_quiet = quiet;
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
            let _ = storage.send(Storage::Sample(engine.readings()));
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
                match target.try_send(job) {
                    Ok(()) => {
                        inflight += 1;
                        due.remove(id);
                    }
                    Err(e) => {
                        let j = e.into_inner();
                        providers.insert(id, j.provider);
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
                cancel.store(true, Ordering::Relaxed);
            }
            Work::Command(Ok(Command::Suspend)) => {
                suspended = true;
                engine.suspend();
            }
            Work::Command(Ok(Command::Resume)) => {
                suspended = false;
                engine.resume();
                hid.invalidate();
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
                for id in hb_providers::provider::FAMILIES
                    .iter()
                    .map(|p| p.0)
                    .chain(["bluetooth", "xinput"])
                {
                    invalidated.insert(id);
                    due.insert(id, Instant::now());
                }
            }
            Work::Command(Ok(Command::Settings(s))) => {
                let remove = engine.settings.status_file && !s.status_file;
                engine.update_settings(s.clone());
                let _ = storage.send(Storage::Save(s));
                if remove {
                    let _ = storage.send(Storage::RemoveStatus);
                }
                for id in providers.keys() {
                    due.insert(id, Instant::now());
                }
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
            Work::ConnectionEvent => {
                hid.invalidate();
                for id in ["bluetooth", "xinput"] {
                    invalidated.insert(id);
                    due.insert(id, Instant::now() + Duration::from_secs(2));
                }
            }
            Work::Completed(Ok(r)) => {
                inflight = inflight.saturating_sub(1);
                let id = r.provider.id();
                diagnostics.insert(id.to_string(), r.provider.diagnostics());
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
                let _ = storage.send(Storage::Sample(engine.readings()));
            }
            Work::Completed(Err(_)) | Work::Idle => {}
        }
        let snapshot = engine.snapshot(clock.unix());
        let _ = events.try_send(Event::Snapshot(snapshot.clone()));
        let _ = events.try_send(Event::Diagnostics(diagnostics.clone()));
        if engine.settings.status_file {
            let _ = storage.send(Storage::Status(snapshot, true));
        }
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
    use super::*;
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
            false
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
    #[test]
    fn status_stays_absent_when_disabled_and_shutdown_drains_queues() {
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
    fn queued_history_flushes_and_status_can_be_disabled() {
        let d = tempfile::tempdir().unwrap();
        let (tx, events, worker) = storage(d.path().to_owned());
        let mut r = Reading::new("mouse", "Mouse", "razer", 100);
        r.level = Some(80);
        tx.send(Storage::Sample(vec![r])).unwrap();
        tx.send(Storage::Status(Snapshot::default(), true)).unwrap();
        tx.send(Storage::RemoveStatus).unwrap();
        tx.send(Storage::History("mouse".into(), 0, 100, 50, 10))
            .unwrap();
        loop {
            if let Event::History(10, result) = events.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                assert_eq!(result.unwrap()[0].level, Some(80));
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
