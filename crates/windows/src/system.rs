//! Owned Windows lifecycle resources and application-specific registration.
use crate::transport::wide;
use hb_core::ProviderError;
use windows::{
    Win32::{
        Foundation::*,
        System::{Registry::*, Threading::*},
        UI::Shell::*,
    },
    core::w,
};
pub struct Instance(HANDLE);
impl Instance {
    pub fn acquire() -> Result<Self, ProviderError> {
        let handle = unsafe { CreateMutexW(None, false, w!("Local\\HaloBatteryNext")) }
            .map_err(|e| ProviderError::new(e.to_string()))?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(ProviderError::new("Halo Battery Next is already running"));
        }
        Ok(Self(handle))
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
pub fn startup(enabled: bool) -> Result<(), ProviderError> {
    let result = if enabled {
        let exe = std::env::current_exe()?;
        if executable_in_temp(
            &exe,
            &[
                std::env::temp_dir(),
                std::env::var_os("TEMP")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_default(),
                std::env::var_os("TMP")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_default(),
            ],
        ) {
            return Err(ProviderError::new(
                "Move Halo Battery Next out of the temporary folder before enabling Start with Windows",
            ));
        }
        let value = wide(&format!("\"{}\" --background", exe.display()));
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
                w!("HaloBatteryNext"),
                REG_SZ.0,
                Some(value.as_ptr().cast()),
                (value.len() * 2) as u32,
            )
        }
    } else {
        unsafe {
            RegDeleteKeyValueW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
                w!("HaloBatteryNext"),
            )
        }
    };
    if result == ERROR_SUCCESS || (!enabled && result == ERROR_FILE_NOT_FOUND) {
        Ok(())
    } else {
        Err(ProviderError::new(format!(
            "Startup registry error {}",
            result.0
        )))
    }
}

fn canonical_components(path: &std::path::Path) -> Option<Vec<String>> {
    if path.as_os_str().is_empty() {
        return None;
    }
    let path = path
        .canonicalize()
        .or_else(|_| std::path::absolute(path))
        .ok()?;
    let mut parts = Vec::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::Prefix(prefix) => {
                // canonicalize produces a verbatim prefix; compare the same
                // normalized drive/UNC prefix as a path that does not exist yet.
                let value = match prefix.kind() {
                    std::path::Prefix::Disk(drive) | std::path::Prefix::VerbatimDisk(drive) => {
                        format!("{}:", char::from(drive).to_ascii_lowercase())
                    }
                    std::path::Prefix::UNC(server, share)
                    | std::path::Prefix::VerbatimUNC(server, share) => format!(
                        "\\\\{}\\{}",
                        server.to_string_lossy().to_lowercase(),
                        share.to_string_lossy().to_lowercase()
                    ),
                    _ => prefix.as_os_str().to_string_lossy().to_lowercase(),
                };
                parts.push(value);
            }
            other => parts.push(other.as_os_str().to_string_lossy().to_lowercase()),
        }
    }
    Some(parts)
}
fn executable_in_temp(exe: &std::path::Path, temp_roots: &[std::path::PathBuf]) -> bool {
    let Some(exe) = canonical_components(exe) else {
        return false;
    };
    temp_roots
        .iter()
        .filter_map(|root| canonical_components(root))
        .any(|root| exe.len() > root.len() && exe.starts_with(&root))
}
pub fn is_startup() -> bool {
    let mut size = 0;
    (unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            w!("HaloBatteryNext"),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    }) == ERROR_SUCCESS
}
pub fn dark_theme() -> bool {
    let mut data = 1u32;
    let mut size = 4;
    unsafe {
        let _ = RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut data as *mut u32).cast()),
            Some(&mut size),
        );
    }
    data == 0
}
pub fn gaming() -> bool {
    unsafe { SHQueryUserNotificationState() }.is_ok_and(|s| {
        s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE || s == QUNS_BUSY
    })
}
pub fn identify() {
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(w!("HaloBatteryNext.Desktop"));
    }
}

/// Native process enumeration, cached for ten seconds like the original tray.
/// This does not spawn PowerShell or require process query privileges.
pub fn mydockfinder_running() -> bool {
    use std::{
        sync::{Mutex, OnceLock},
        time::{Duration, Instant},
    };
    use windows::Win32::System::Diagnostics::ToolHelp::*;
    static CACHE: OnceLock<Mutex<Option<(Instant, bool)>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if let Some((at, value)) = *cache
        && at.elapsed() < Duration::from_secs(10)
    {
        return value;
    }
    struct Snapshot(HANDLE);
    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    let value = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .ok()
        .map(|h| {
            let snapshot = Snapshot(h);
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut present = unsafe { Process32FirstW(snapshot.0, &mut entry) }.is_ok();
            while present {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|x| *x == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                if is_mydockfinder(&name) {
                    return true;
                }
                present = unsafe { Process32NextW(snapshot.0, &mut entry) }.is_ok();
            }
            false
        })
        .unwrap_or(false);
    *cache = Some((Instant::now(), value));
    value
}
fn is_mydockfinder(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "dock_64.exe" | "dock_32.exe" | "mydockfinder.exe"
    ) || name.contains("mydock")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_process_names() {
        assert!(is_mydockfinder("Dock_64.EXE"));
        assert!(is_mydockfinder("MyDockBeta.exe"));
        assert!(!is_mydockfinder("explorer.exe"));
    }
    #[test]
    fn temporary_executable_copy_and_directory_boundaries() {
        use std::path::{Path, PathBuf};
        let roots = [
            PathBuf::from("C:\\Users\\Tester\\Temp"),
            PathBuf::from("D:\\Tmp"),
        ];
        assert!(executable_in_temp(
            Path::new("C:\\Users\\Tester\\Temp\\Rar$EXa1234\\HaloBatteryNext.exe"),
            &roots
        ));
        assert!(executable_in_temp(
            Path::new("d:\\TMP\\extract\\HaloBatteryNext.exe"),
            &roots
        ));
        assert!(!executable_in_temp(
            Path::new("C:\\Users\\Tester\\TempTools\\HaloBatteryNext.exe"),
            &roots
        ));
        assert!(!executable_in_temp(
            Path::new("C:\\Tools\\HaloBatteryNext.exe"),
            &roots
        ));
        assert!(!executable_in_temp(
            Path::new("C:\\Users\\Tester\\Temp\\..\\Tools\\HaloBatteryNext.exe"),
            &roots
        ));
        assert!(!executable_in_temp(
            Path::new("C:\\Tools\\HaloBatteryNext.exe"),
            &[PathBuf::new()]
        ));
    }
}
