//! Explicit hardware configuration, serialized by the application's HID owner.
use crate::{provider::trusted_identity, razer_controls};
use hb_core::*;
use std::time::Duration;

/// Configuration routes are independent of the battery catalog. Adding battery
/// support must never grant permission to send configuration commands.
pub const POLLING_PROVIDERS: &[&str] = &["razer", "logitech", "mchose"];

#[derive(Default)]
pub struct HidDeviceController;

/// Enforce the enumeration epoch and cancellation before and after every call,
/// including every write in a multi-packet operation.
pub(crate) struct GuardedSession<'a> {
    session: Box<dyn HidSession>,
    hid: &'a dyn HidTransport,
    context: &'a PollContext<'a>,
    generation: u64,
}
impl<'a> GuardedSession<'a> {
    pub(crate) fn new(
        session: Box<dyn HidSession>,
        hid: &'a dyn HidTransport,
        context: &'a PollContext<'a>,
        generation: u64,
    ) -> Self {
        Self {
            session,
            hid,
            context,
            generation,
        }
    }
    fn active(&self) -> Result<(), ProviderError> {
        if !self.context.active() {
            return Err(ProviderError::new(
                "configuration cancelled or deadline reached",
            ));
        }
        if self.hid.generation() != self.generation {
            return Err(ProviderError::new(
                "device connection changed; refresh configuration before applying",
            ));
        }
        Ok(())
    }
}
impl HidSession for GuardedSession<'_> {
    fn write(&mut self, d: &[u8]) -> Result<(), ProviderError> {
        self.active()?;
        let result = self.session.write(d);
        self.active()?;
        result
    }
    fn read(&mut self, n: usize, t: Duration) -> Result<Vec<u8>, ProviderError> {
        self.active()?;
        let result = self.session.read(n, t);
        self.active()?;
        result
    }
    fn send_feature(&mut self, d: &[u8]) -> Result<(), ProviderError> {
        self.active()?;
        let result = self.session.send_feature(d);
        self.active()?;
        result
    }
    fn feature(&mut self, id: u8, n: usize) -> Result<Vec<u8>, ProviderError> {
        self.active()?;
        let result = self.session.feature(id, n);
        self.active()?;
        result
    }
    fn input_report(&mut self, id: u8, n: usize) -> Result<Vec<u8>, ProviderError> {
        self.active()?;
        let result = self.session.input_report(id, n);
        self.active()?;
        result
    }
}
pub(crate) fn rate(hz: Option<u32>) -> Option<PollingRate> {
    hz.and_then(|h| PollingRate::try_from(h).ok())
}

impl DeviceController for HidDeviceController {
    fn execute(
        &mut self,
        request: &ControlRequest,
        hid: &dyn HidTransport,
        context: &PollContext<'_>,
    ) -> ControlOutcome {
        let reading = &request.target.reading;
        if reading.source == "logitech" {
            return crate::logitech_adapter::execute(request, hid, context);
        }
        if reading.source == "mchose" {
            return crate::mchose_adapter::execute(request, hid, context);
        }
        if reading.source != "razer" {
            return ControlOutcome::failed(
                request,
                "hardware polling controls are unavailable for this provider",
            );
        }
        if !reading.online() || reading.kind != "mouse" {
            return ControlOutcome::failed(request, "select an online mouse to configure");
        }
        if !context.active() {
            return ControlOutcome::failed(request, "configuration cancelled or deadline reached");
        }
        let generation = hid.generation();
        if matches!(request.action, ControlAction::Apply(_))
            && request.target.generation != generation
        {
            return ControlOutcome::failed(
                request,
                "device connection changed; refresh configuration before applying",
            );
        }
        let devices = match hid.enumerate(0x1532) {
            Ok(d) => d,
            Err(e) => return ControlOutcome::failed(request, e.message),
        };
        let mut matching = devices.iter().filter(|info| {
            razer_controls::protocol(info).is_some()
                && reading.key
                    == format!("razer:{:04x}:{}", info.product_id, trusted_identity(info))
        });
        let Some(info) = matching.next() else {
            return ControlOutcome::failed(
                request,
                "supported control collection for this exact device is unavailable",
            );
        };
        if matching.next().is_some() {
            return ControlOutcome::failed(
                request,
                "multiple control collections match this identity; refusing ambiguous target",
            );
        }
        // Display names are not identities. Preserve independently observed
        // serial/container values even when the stable key uses only one.
        if reading
            .serial
            .as_ref()
            .is_some_and(|s| !s.trim().eq_ignore_ascii_case(info.serial.trim()))
            || reading.container.as_ref().is_some_and(|c| {
                info.container
                    .as_ref()
                    .is_none_or(|i| !c.trim().eq_ignore_ascii_case(i.trim()))
            })
        {
            return ControlOutcome::failed(
                request,
                "device serial or container changed; refresh configuration before applying",
            );
        }
        let protocol = razer_controls::protocol(info).unwrap();
        let requested = match request.action {
            ControlAction::Read => None,
            ControlAction::Apply(r) => Some(r.hz()),
        };
        if requested.is_some_and(|r| !protocol.rates().contains(&r)) {
            return ControlOutcome::failed(request, "polling rate is unsupported on this device");
        }
        if !context.active() || hid.generation() != generation {
            return ControlOutcome::failed(
                request,
                "device connection changed or configuration cancelled before opening",
            );
        }
        let session = match hid.open(info) {
            Ok(s) => s,
            Err(e) => return ControlOutcome::failed(request, e.message),
        };
        let mut guarded = GuardedSession::new(session, hid, context, generation);
        let result = razer_controls::execute_rate(&mut guarded, context, protocol, requested);
        let failure = result
            .failure
            .map(|f| format!("Razer polling configuration failed: {f:?}"));
        let observation = result.observed_hz.map(|hz| PollingObservation {
            target: ControlTarget {
                reading: reading.clone(),
                generation,
            },
            supported: protocol
                .rates()
                .iter()
                .filter_map(|h| PollingRate::try_from(*h).ok())
                .collect(),
            rate: rate(Some(hz)),
            timestamp: context.clock.unix(),
            evidence: match protocol {
                razer_controls::Protocol::Extended | razer_controls::Protocol::ExtendedWireless => {
                    "OpenRazer high-rate protocol reference; hardware unverified locally"
                }
                razer_controls::Protocol::Legacy => {
                    "OpenMouse legacy polling reference; hardware unverified locally"
                }
            }
            .into(),
        });
        ControlOutcome {
            request: request.request,
            key: reading.key.clone(),
            observation,
            previous: rate(result.previous_hz),
            may_have_changed: result.may_have_changed,
            failure,
        }
    }
}
