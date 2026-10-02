//! Optional, explicit device configuration. No game or input-event access.
use crate::{Connection, HidTransport, PollContext, Reading};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct PollingRate(u32);
impl PollingRate {
    pub fn hz(self) -> u32 {
        self.0
    }
}
impl TryFrom<u32> for PollingRate {
    type Error = &'static str;
    fn try_from(hz: u32) -> Result<Self, Self::Error> {
        match hz {
            125 | 250 | 500 | 1000 | 2000 | 4000 | 8000 => Ok(Self(hz)),
            _ => Err("unsupported polling rate"),
        }
    }
}
impl From<PollingRate> for u32 {
    fn from(rate: PollingRate) -> Self {
        rate.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PollingCapability {
    ReadWrite,
    Unavailable(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigurationDevice {
    pub key: String,
    pub name: String,
    pub kind: String,
    pub source: String,
    pub via: String,
    pub connection: Connection,
    pub serial: Option<String>,
    pub container: Option<String>,
    pub capability: PollingCapability,
}
impl ConfigurationDevice {
    pub fn from_reading(reading: &Reading) -> Self {
        Self {
            key: reading.key.clone(),
            name: reading.name.clone(),
            kind: reading.kind.clone(),
            source: reading.source.clone(),
            via: reading.via.clone(),
            connection: reading.connection.clone(),
            serial: reading.serial.clone(),
            container: reading.container.clone(),
            capability: PollingCapability::ReadWrite,
        }
    }
    pub fn online(&self) -> bool {
        self.connection == Connection::Online
    }
    pub fn matches_reading(&self, reading: &Reading) -> bool {
        self.key == reading.key
            && self.kind == reading.kind
            && self.source == reading.source
            && self.via == reading.via
            && self.serial == reading.serial
            && self.container == reading.container
    }
}
#[derive(Clone, Debug)]
pub struct ControlTarget {
    pub device: ConfigurationDevice,
    /// Enumeration epoch of the last successful configuration read.
    pub generation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlAction {
    Read,
    Apply(PollingRate),
}
#[derive(Clone, Debug)]
pub struct ControlRequest {
    pub request: u64,
    pub target: ControlTarget,
    pub action: ControlAction,
}
#[derive(Clone, Debug)]
pub struct PollingObservation {
    pub target: ControlTarget,
    pub supported: Vec<PollingRate>,
    pub rate: Option<PollingRate>,
    pub timestamp: i64,
    pub evidence: String,
}
#[derive(Clone, Debug)]
pub struct ControlOutcome {
    pub request: u64,
    pub key: String,
    pub observation: Option<PollingObservation>,
    pub previous: Option<PollingRate>,
    pub may_have_changed: bool,
    pub failure: Option<String>,
}
impl ControlOutcome {
    pub fn failed(request: &ControlRequest, message: impl Into<String>) -> Self {
        Self {
            request: request.request,
            key: request.target.device.key.clone(),
            observation: None,
            previous: None,
            may_have_changed: false,
            failure: Some(message.into()),
        }
    }
    pub fn confirmed_change(&self) -> bool {
        self.failure.is_none()
            && self.may_have_changed
            && self
                .observation
                .as_ref()
                .and_then(|o| o.rate)
                .is_some_and(|rate| Some(rate) != self.previous)
    }
}
pub trait DeviceController: Send {
    fn execute(
        &mut self,
        request: &ControlRequest,
        hid: &dyn HidTransport,
        context: &PollContext<'_>,
    ) -> ControlOutcome;
}
