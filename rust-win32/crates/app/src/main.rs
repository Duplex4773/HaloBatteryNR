#![windows_subsystem = "windows"]
mod chart;
mod icons;
mod runtime;
mod storage_worker;
mod ui;
use hb_core::*;
use std::{path::PathBuf, sync::atomic::AtomicBool, time::Duration};
fn main() {
    if let Err(e) = entry() {
        if std::env::args().any(|a| a == "--probe") {
            eprintln!("Halo Battery Next: {e}");
            std::process::exit(1);
        }
        let message = hb_windows::transport::wide(&e.to_string());
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
                None,
                windows::core::PCWSTR(message.as_ptr()),
                windows::core::w!("Halo Battery Next"),
                windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
            );
        }
        std::process::exit(1);
    }
}
fn entry() -> Result<(), ProviderError> {
    let args: Vec<String> = std::env::args().collect();
    let option = |name: &str| {
        args.iter()
            .position(|s| s == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let dir = option("--data-dir")
        .map(PathBuf::from)
        .unwrap_or_else(hb_storage::data_dir);
    if args.iter().any(|a| a == "--probe") {
        let hid = hb_windows::WindowsHid::new()?;
        let clock = SystemClock::default();
        let cancel = AtomicBool::new(false);
        let mut providers = hb_providers::providers();
        providers.push(Box::new(hb_windows::BluetoothProvider::default()));
        providers.push(Box::new(hb_windows::ControllerProvider));
        if let Some(selected) = option("--provider")
            && !providers.iter().any(|p| p.id() == selected)
        {
            return Err(ProviderError::new(format!("Unknown provider: {selected}")));
        }
        let _ = unsafe {
            windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_MULTITHREADED,
            )
        };
        let mut output = serde_json::Map::new();
        for mut p in providers {
            if option("--provider").is_some_and(|id| id != p.id()) {
                continue;
            }
            let c = PollContext {
                clock: &clock,
                cancelled: &cancel,
                deadline: clock.monotonic() + Duration::from_secs(25),
                playstation_full_mode: false,
            };
            let result = p.poll(&hid, &c);
            output.insert(
                p.id().into(),
                match result {
                    Ok(r) => serde_json::json!({"devices":r,"diagnostics":p.diagnostics()}),
                    Err(e) => {
                        serde_json::json!({"error":e.to_string(),"diagnostics":p.diagnostics()})
                    }
                },
            );
        }
        let path = option("--output")
            .map(PathBuf::from)
            .unwrap_or_else(|| dir.join("probe.json"));
        hb_storage::atomic_write(&path, &serde_json::to_vec_pretty(&output).unwrap())?;
        return Ok(());
    }
    let _instance = match hb_windows::system::Instance::acquire() {
        Ok(instance) => instance,
        Err(e) => {
            // A second normal launch opens the existing dashboard. The mutex
            // still owns lifecycle exclusion; this lookup never starts workers.
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::*;
                if let Ok(hwnd) = FindWindowW(
                    windows::core::w!("HaloBatteryNext.Native"),
                    windows::core::w!("Halo Battery Next monitor"),
                ) {
                    if !args.iter().any(|a| a == "--background") {
                        PostMessageW(
                            Some(hwnd),
                            WM_APP + 8,
                            windows::Win32::Foundation::WPARAM(0),
                            windows::Win32::Foundation::LPARAM(0),
                        )
                        .map_err(|e| ProviderError::new(e.to_string()))?;
                    }
                    return Ok(());
                }
            }
            return Err(e);
        }
    };
    hb_windows::system::identify();
    let settings = hb_storage::load_settings(&dir.join("config.json"));
    let runtime = runtime::Runtime::start(
        dir.clone(),
        settings.clone(),
        args.iter().any(|a| a == "--simulate"),
    )?;
    ui::run(
        runtime,
        settings,
        dir,
        args.iter().any(|a| a == "--background"),
    )
}
