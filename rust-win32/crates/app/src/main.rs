#![windows_subsystem = "windows"]
mod chart;
mod icons;
mod runtime;
mod storage_worker;
mod ui;
use hb_core::*;
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
fn main() {
    if let Err(e) = entry() {
        if std::env::args().any(|a| a == "--probe" || a == "--polling-probe") {
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
    if args.iter().any(|a| a == "--polling-probe") {
        // Diagnostic access is exclusive with the running app. Writes always
        // require an explicit rate and exact stable key; no default mouse SET.
        let _instance = hb_windows::system::Instance::acquire()?;
        let selected_provider = option("--provider").unwrap_or_else(|| "razer".into());
        if !["razer", "logitech"].contains(&selected_provider.as_str()) {
            return Err(ProviderError::new(
                "Polling controls support Razer and Logitech only",
            ));
        }
        let action = match option("--polling-hz") {
            Some(hz) => {
                if option("--device-key").is_none() {
                    return Err(ProviderError::new(
                        "An explicit --device-key is required for a rate change",
                    ));
                }
                let hz = hz
                    .parse::<u32>()
                    .map_err(|_| ProviderError::new("Invalid polling rate"))?;
                ControlAction::Apply(PollingRate::try_from(hz).map_err(ProviderError::new)?)
            }
            None => ControlAction::Read,
        };
        if matches!(action, ControlAction::Apply(_)) && hb_windows::system::polling_apply_blocked()
        {
            return Err(ProviderError::new(
                "Close the game or presentation and verify the Windows notification state before changing the rate",
            ));
        }
        let hid = hb_windows::WindowsHid::new()?;
        let clock = SystemClock::default();
        let cancel = Arc::new(AtomicBool::new(false));
        let context = PollContext {
            clock: &clock,
            cancelled: &cancel,
            deadline: clock.monotonic() + Duration::from_secs(25),
            playstation_full_mode: false,
        };
        let mut provider = hb_providers::providers()
            .into_iter()
            .find(|p| p.id() == selected_provider)
            .unwrap();
        let devices = provider.poll(&hid, &context)?;
        let mut output = Vec::new();
        for reading in devices
            .into_iter()
            .filter(|r| option("--device-key").is_none_or(|key| key == r.key))
        {
            let request = ControlRequest {
                request: 1,
                target: ControlTarget {
                    reading,
                    generation: hid.generation(),
                },
                action,
            };
            let result = runtime::execute_control(
                &request,
                &hid,
                &clock,
                &cancel,
                &cancel,
                hb_windows::system::polling_apply_blocked(),
                None,
            );
            output.push(serde_json::json!({ "key": result.key,
                "name": request.target.reading.name,
                "previous_hz": result.previous.map(PollingRate::hz),
                "observed_hz": result.observation.as_ref().and_then(|o| o.rate).map(PollingRate::hz),
                "supported_hz": result.observation.as_ref().map(|o| o.supported.iter().map(|r| r.hz()).collect::<Vec<_>>()),
                "timestamp": result.observation.as_ref().map(|o| o.timestamp),
                "may_have_changed": result.may_have_changed,
                "failure": result.failure }));
        }
        if output.is_empty() {
            return Err(ProviderError::new("No matching online device"));
        }
        let path = option("--output")
            .map(PathBuf::from)
            .unwrap_or_else(|| dir.join("polling-probe.json"));
        hb_storage::atomic_write(&path, &serde_json::to_vec_pretty(&output).unwrap())?;
        return Ok(());
    }
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
    let icon_path = dir.join("application.png");
    let icon: &[u8] = include_bytes!("../application.png");
    let registration = (|| {
        hb_windows::system::identify();
        if std::fs::read(&icon_path).ok().as_deref() != Some(icon) {
            hb_storage::atomic_write(&icon_path, icon)?;
        }
        hb_windows::system::identify_registered_with_icon(&icon_path)
    })();
    let identity_error = registration.err().map(|e| e.to_string());
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
        identity_error,
    )
}
