use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DevicePreferences {
    pub name: Option<String>,
    pub hidden: bool,
    pub icon: Option<String>,
    pub low: Option<u8>,
    pub requested_polling_rate: Option<crate::PollingRate>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub interval: u64,
    pub low: u8,
    /// Visual warning threshold; zero disables orange. Red low-alert color has priority.
    pub warning_level: u8,
    pub notify: bool,
    pub full_alert: bool,
    pub bluetooth: bool,
    pub animation: bool,
    pub badges: bool,
    pub icon_theme: String,
    pub fluent_menu: bool,
    pub time_left: bool,
    pub percent_in_icon: bool,
    pub quiet_fullscreen: bool,
    pub status_file: bool,
    pub playstation_full_mode: bool,
    pub polling_controls: bool,
    pub disabled_providers: BTreeSet<String>,
    pub devices: BTreeMap<String, DevicePreferences>,
    pub update_check: bool,
    pub release_repository: Option<String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            interval: 60,
            low: 20,
            warning_level: 30,
            notify: true,
            full_alert: true,
            bluetooth: true,
            animation: true,
            badges: true,
            icon_theme: "auto".into(),
            fluent_menu: true,
            time_left: true,
            percent_in_icon: false,
            quiet_fullscreen: true,
            status_file: false,
            playstation_full_mode: false,
            polling_controls: false,
            disabled_providers: BTreeSet::new(),
            devices: BTreeMap::new(),
            update_check: false,
            release_repository: None,
        }
    }
}
impl Settings {
    /// Invalid individual properties retain their defaults rather than invalidating
    /// the entire file. Unknown properties are ignored for forward compatibility.
    pub fn from_value(value: serde_json::Value) -> Result<Self, String> {
        let obj = value.as_object().ok_or("settings must be a JSON object")?;
        let mut base = serde_json::to_value(Self::default()).map_err(|e| e.to_string())?;
        for (key, val) in obj {
            let Some(default) = base.get(key) else {
                continue;
            };
            let valid = match key.as_str() {
                "interval" => val.as_u64().is_some_and(|n| (5..=3600).contains(&n)),
                "low" | "warning_level" => val.as_u64().is_some_and(|n| n <= 100),
                "icon_theme" => val
                    .as_str()
                    .is_some_and(|s| ["auto", "white", "black", "windows", "topbar"].contains(&s)),
                "disabled_providers" => {
                    serde_json::from_value::<BTreeSet<String>>(val.clone()).is_ok()
                }
                "devices" => val.is_object(),
                "release_repository" => val.is_null() || val.as_str().is_some_and(valid_repository),
                _ => {
                    (default.is_boolean() && val.is_boolean())
                        || (default.is_string() && val.is_string())
                }
            };
            if valid {
                base[key] = if key == "devices" {
                    let devices: BTreeMap<String, DevicePreferences> = val
                        .as_object()
                        .unwrap()
                        .iter()
                        .filter_map(|(key, value)| {
                            let fields = value.as_object()?;
                            Some((
                                key.clone(),
                                DevicePreferences {
                                    name: fields
                                        .get("name")
                                        .and_then(|v| v.as_str())
                                        .map(str::to_owned),
                                    hidden: fields
                                        .get("hidden")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false),
                                    icon: fields
                                        .get("icon")
                                        .and_then(|v| v.as_str())
                                        .map(str::to_owned),
                                    low: fields
                                        .get("low")
                                        .and_then(|v| v.as_u64())
                                        .filter(|n| *n <= 100)
                                        .map(|n| n as u8),
                                    requested_polling_rate: fields
                                        .get("requested_polling_rate")
                                        .and_then(|v| v.as_u64())
                                        .and_then(|n| u32::try_from(n).ok())
                                        .and_then(|n| crate::PollingRate::try_from(n).ok()),
                                },
                            ))
                        })
                        .collect();
                    serde_json::to_value(devices).map_err(|e| e.to_string())?
                } else {
                    val.clone()
                };
            }
        }
        let mut settings: Self = serde_json::from_value(base).map_err(|e| e.to_string())?;
        for d in settings.devices.values_mut() {
            if d.low.is_some_and(|n| n > 100) {
                d.low = None;
            }
            if d.icon.as_deref().is_some_and(|s| {
                ![
                    "mouse",
                    "headset",
                    "keyboard",
                    "gamepad",
                    "bluetooth",
                    "dualshock",
                    "dualsense",
                ]
                .contains(&s)
            }) {
                d.icon = None;
            }
            if let Some(name) = &mut d.name {
                name.truncate(name.char_indices().nth(120).map_or(name.len(), |(i, _)| i));
            }
        }
        if settings.release_repository.is_none() {
            settings.update_check = false;
        }
        Ok(settings)
    }
    pub fn low_for(&self, key: &str) -> u8 {
        self.devices
            .get(key)
            .and_then(|d| d.low)
            .unwrap_or(self.low)
    }
    pub fn enabled(&self, provider: &str) -> bool {
        !self.disabled_providers.contains(provider) && (provider != "bluetooth" || self.bluetooth)
    }
}
pub fn valid_repository(s: &str) -> bool {
    let parts: Vec<_> = s.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && *p != "."
                && *p != ".."
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
}
