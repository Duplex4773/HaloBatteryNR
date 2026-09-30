//! A single UI thread owns all HWND, HICON and Direct2D resources.
use crate::{
    chart::Chart,
    icons::{self, Icon},
    runtime::{Command, Event, Runtime},
};
use hb_core::*;
use std::{cell::RefCell, collections::BTreeMap, path::PathBuf};
use windows::{
    Win32::{
        Devices::HumanInterfaceDevice::HidD_GetHidGuid,
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::*, Shell::*, WindowsAndMessaging::*},
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
struct Tray {
    data: NOTIFYICONDATAW,
    frames: Vec<Icon>,
    signature: String,
    frame: usize,
}
impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data);
        }
    }
}
struct State {
    context: *const RefCell<State>,
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
    points: Vec<Reading>,
    since: i64,
    until: i64,
    request: u64,
    days: i64,
    taskbar: u32,
    notify: Option<HDEVNOTIFY>,
    animating: bool,
    error: String,
    font: HFONT,
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
        let context = Box::new(RefCell::new(State {
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
            points: vec![],
            since: 0,
            until: 0,
            request: 0,
            days: 1,
            taskbar: RegisterWindowMessageW(w!("TaskbarCreated")),
            notify: None,
            animating: false,
            error: String::new(),
            font,
        }));
        let ptr = &*context as *const RefCell<State>;
        let mut state = context.borrow_mut();
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
        state.sync_trays(false);
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
            let dashboard = context.borrow().dashboard;
            if message.message == WM_SYSCHAR && b"dhsr".contains(&(message.wParam.0 as u8)) {
                DispatchMessageW(&message);
                continue;
            }
            if dashboard.is_some_and(|h| IsDialogMessageW(h, &message).as_bool()) {
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let mut state = context.borrow_mut();
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
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<State>;
        if ptr.is_null() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        if msg == WM_NCDESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        let Ok(mut guard) = (&*ptr).try_borrow_mut() else {
            return DefWindowProcW(hwnd, msg, wp, lp);
        };
        let s = &mut *guard;
        if msg == s.taskbar {
            s.sync_trays(true);
            return LRESULT(0);
        }
        match msg {
            m if m == WM_APP + 7 => {
                s.drain();
                LRESULT(0)
            }
            m if m == WM_APP + 8 => {
                s.open();
                LRESULT(0)
            }
            m if m == WM_APP + 9 => {
                if let Some(h) = s.dashboard {
                    let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
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
                    s.selected = i;
                }
                let event = (lp.0 as u32) & 0xffff;
                if event == WM_CONTEXTMENU || event == WM_RBUTTONUP {
                    s.menu(key.clone())
                } else if event == WM_LBUTTONUP || event == NIN_SELECT || event == (NIN_SELECT | 1)
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
                    s.chart = None;
                    s.controls.clear();
                    s.dashboard = None;
                    let _ = DeleteObject(s.font.into());
                    s.font = HFONT::default();
                    let _ = DestroyWindow(hwnd);
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
                s.sync_trays(true);
                LRESULT(0)
            }
            WM_SETTINGCHANGE => {
                s.sync_trays(true);
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
                        && c.paint(r.right as u32, r.bottom as u32, &s.points, s.since, s.until)
                            .is_err()
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
            _ => DefWindowProcW(hwnd, msg, wp, lp),
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
            WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
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
                let _ = ShowWindow(h, SW_RESTORE);
                let _ = SetForegroundWindow(h);
                return;
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
                    self.build();
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
                    self.label(93, "Low alert % (0 disables)", 20, 270, 230);
                    self.edit(13, &d.low_alert_at.to_string(), 260, 266, 80);
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
                self.label(95, &self.error.clone(), 20, 420, 750);
                self.button(5, "Export &diagnostics", 20, 470, 200);
            }
            2 => {
                self.combo(10, &names, self.selected, 20, 65, 530);
                self.combo(
                    20,
                    &["24 hours".into(), "7 days".into(), "30 days".into()],
                    match self.days {
                        7 => 1,
                        30 => 2,
                        _ => 0,
                    },
                    570,
                    65,
                    210,
                );
                self.label(
                    96,
                    "Battery level: 0–100% · gaps represent unavailable readings",
                    20,
                    105,
                    750,
                );
                self.label(97,"History uses the selected interval and graph width. Refresh to load latest samples.",20,735,760);
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
                self.label(200, "Poll seconds (5–3600)", 20, 270, 230);
                self.edit(201, &self.settings.interval.to_string(), 260, 266, 100);
                self.label(202, "Default low alert %", 410, 270, 210);
                self.edit(203, &self.settings.low.to_string(), 660, 266, 80);
                let themes: Vec<_> = ["auto", "white", "black", "windows", "topbar"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                self.label(204, "Icon theme", 20, 310, 150);
                self.combo(
                    205,
                    &themes,
                    themes
                        .iter()
                        .position(|s| s == &self.settings.icon_theme)
                        .unwrap_or(0),
                    180,
                    306,
                    180,
                );
                self.check(
                    206,
                    "Launch at sign in",
                    hb_windows::system::is_startup(),
                    410,
                    306,
                    360,
                );
                self.label(207, "Provider switches", 20, 350, 740);
                for (i, p) in providers().iter().enumerate() {
                    self.check(
                        300 + i as u16,
                        p,
                        self.settings.enabled(p),
                        20 + (i as i32 % 4) * 190,
                        380 + (i as i32 / 4) * 30,
                        180,
                    );
                }
                self.label(208, "Release repository (owner/name)", 20, 604, 300);
                self.edit(
                    209,
                    self.settings
                        .release_repository
                        .clone()
                        .as_deref()
                        .unwrap_or(""),
                    330,
                    600,
                    410,
                );
                self.button(210, "&Save settings", 20, 650, 180);
                self.button(5, "Export &diagnostics", 220, 650, 200);
                self.label(95, &self.error.clone(), 20, 700, 750);
            }
            _ => {}
        }
        unsafe {
            let _ = InvalidateRect(self.dashboard, None, true);
        }
    }
    fn save(&mut self) {
        self.runtime.send(Command::Settings(self.settings.clone()));
        self.sync_trays(true);
    }
    fn command(&mut self, id: u16, notification: u16) {
        match id {
            1..=3 => {
                self.page = id;
                self.build()
            }
            4 => {
                self.runtime.send(Command::Refresh);
                if self.page == 2 {
                    self.query()
                }
            }
            5 => {
                let path = self.dir.join("diagnostics.json");
                let value = serde_json::json!({"snapshot":self.snapshot,"providers":self.diagnostics,"settings":self.settings});
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
                self.selected = self.choice(10);
                self.build()
            }
            20 if notification == CBN_SELCHANGE as u16 => {
                self.days = [1, 7, 30][self.choice(20).min(2)];
                self.query()
            }
            15 => {
                if let Some(d) = self.snapshot.devices.get(self.selected) {
                    let key = d.reading.key.clone();
                    let name = self.text(11);
                    let hidden = self.checked(12);
                    let low = self.text(13).parse::<u8>().ok().filter(|v| *v <= 100);
                    if low.is_none() {
                        self.error = "Enter a low alert value between 0 and 100".into();
                        self.build();
                        return;
                    }
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
                        key,
                        DevicePreferences {
                            name: (!name.trim().is_empty()).then_some(name),
                            hidden,
                            icon,
                            low,
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
                if interval.is_none() || low.is_none() {
                    self.error = "Poll seconds must be 5–3600; alert must be 0–100".into();
                    self.build();
                    return;
                }
                value["interval"] = interval.unwrap().into();
                value["low"] = low.unwrap().into();
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
    fn query(&mut self) {
        let Some(d) = self.snapshot.devices.get(self.selected) else {
            return;
        };
        self.until = SystemClock::default().unix();
        self.since = self.until - self.days * 86400;
        self.request += 1;
        let mut r = RECT::default();
        unsafe {
            if let Some(h) = self.dashboard {
                let _ = GetClientRect(h, &mut r);
            }
        }
        self.runtime.send(Command::History {
            key: d.reading.key.clone(),
            since: self.since,
            until: self.until,
            width: (r.right - 80).max(20) as usize,
            request: self.request,
        });
    }
    fn drain(&mut self) {
        let mut latest = None;
        while let Ok(e) = self.runtime.events.try_recv() {
            match e {
                Event::Snapshot(s) => latest = Some(s),
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
                Event::History(id, result) if id == self.request => {
                    match result {
                        Ok(points) => self.points = points,
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
            self.snapshot = s;
            if self.page == 1
                && let Some(d) = self.snapshot.devices.get(self.selected)
                && let Some(h) = self.controls.get(&91)
            {
                let text = wide(&device_detail(d));
                unsafe {
                    let _ = SetWindowTextW(*h, PCWSTR(text.as_ptr()));
                }
            }
            self.sync_trays(false);
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
    }
    fn sync_trays(&mut self, force: bool) {
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
            let signature = format!(
                "{:?}{:?}{:?}{}{}{}{:?}{:?}",
                d.reading.level,
                d.reading.charging,
                d.reading.connection,
                d.icon,
                d.low_alert_at,
                dark,
                self.settings.icon_theme,
                (
                    self.settings.animation,
                    self.settings.percent_in_icon,
                    self.settings.badges
                )
            );
            let tip = d.text.clone();
            let existing = self.trays.get_mut(&d.reading.key);
            if let Some(t) = existing {
                let mut changed = force;
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
                if changed {
                    t.data.hIcon = t.frames[0].0;
                    unsafe {
                        if force {
                            let _ = Shell_NotifyIconW(NIM_DELETE, &t.data);
                        }
                        let _ =
                            Shell_NotifyIconW(if force { NIM_ADD } else { NIM_MODIFY }, &t.data);
                    }
                }
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
                unsafe {
                    let _ = Shell_NotifyIconW(NIM_ADD, &data);
                }
                self.trays.insert(
                    d.reading.key.clone(),
                    Tray {
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
        let _ = AppendMenuW(menu.0, MF_STRING, 502, w!("E&xit Halo Battery"));

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
