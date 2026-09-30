use crate::{
    catalog::{AUDEZE_REQUESTS, DEVICES, Device},
    protocols::{self as p, Battery},
};
use hb_core::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

pub const FAMILIES: &[(&str, &str)] = &[
    ("razer", "Razer mice and headsets"),
    ("audeze", "Audeze Maxwell"),
    ("wlmouse", "WLmouse"),
    ("mchose", "MCHOSE"),
    ("hyperx_alpha2", "HyperX Cloud Alpha 2"),
    ("hyperx_cloud3", "HyperX Cloud III Wireless"),
    ("hyperx", "HyperX Cloud II Wireless"),
    ("keychron", "Keychron"),
    ("pulsar", "Pulsar / ATK / VXE / Hitscan"),
    ("jbl", "JBL Quantum"),
    ("logitech", "Logitech"),
    ("steelseries", "SteelSeries"),
    ("playstation", "PlayStation controllers"),
    ("eightbitdo", "8BitDo controllers"),
    ("barracuda", "Razer Barracuda Pro"),
    ("nintendo", "Nintendo Switch controllers"),
    ("asus", "ASUS ROG / TUF"),
    ("gwolves", "G-Wolves"),
    ("lofree", "Lofree"),
    ("astro", "Astro A50"),
    ("corsair", "Corsair"),
    ("lamzu", "LAMZU"),
    ("am_infinity", "AM Infinity 8K"),
];
pub fn providers() -> Vec<Box<dyn BatteryProvider>> {
    FAMILIES
        .iter()
        .map(|(id, _)| Box::new(HidProvider::new(id)) as Box<dyn BatteryProvider>)
        .collect()
}
pub struct HidProvider {
    id: &'static str,
    diagnostics: Vec<String>,
    last: BTreeMap<String, Reading>,
    first_seen: BTreeMap<String, i64>,
    stuck: BTreeMap<String, u8>,
    logitech_identity: BTreeMap<String, (String, String, String)>,
    logitech_asleep: BTreeSet<String>,
    logitech_slots: BTreeMap<String, String>,
    logitech_error: Option<u8>,
    logitech_timeout: u64,
    audeze_short_useless: BTreeSet<String>,
    audeze_echo: bool,
    pa_accepted: bool,
    razer_dead: BTreeMap<String, i64>,
    steel_path: BTreeMap<u16, String>,
    family_names: BTreeMap<String, String>,
    mchose_models: BTreeMap<u16, u16>,
    last_reply_accepted: bool,
    counter: u8,
    razer_status: u16,
    razer_cache: BTreeMap<String, (String, u8)>,
}
impl HidProvider {
    pub fn new(id: &'static str) -> Self {
        Self {
            id,
            diagnostics: Vec::new(),
            last: BTreeMap::new(),
            first_seen: BTreeMap::new(),
            stuck: BTreeMap::new(),
            logitech_identity: BTreeMap::new(),
            logitech_asleep: BTreeSet::new(),
            logitech_slots: BTreeMap::new(),
            logitech_error: None,
            logitech_timeout: 600,
            audeze_short_useless: BTreeSet::new(),
            audeze_echo: false,
            pa_accepted: false,
            razer_dead: BTreeMap::new(),
            steel_path: BTreeMap::new(),
            family_names: BTreeMap::new(),
            mchose_models: BTreeMap::new(),
            last_reply_accepted: false,
            counter: 0,
            razer_status: 0,
            razer_cache: BTreeMap::new(),
        }
    }
    fn log(&mut self, text: impl Into<String>) {
        if self.diagnostics.len() < 120 {
            self.diagnostics.push(text.into());
        }
    }
    fn receive(
        &mut self,
        s: &mut dyn HidSession,
        c: &PollContext<'_>,
        attempts: usize,
        ms: u64,
        accept: impl Fn(&[u8]) -> bool,
    ) -> Result<Option<Vec<u8>>, ProviderError> {
        for _ in 0..attempts {
            if !c.active() {
                break;
            }
            let r = s.read(
                64,
                Duration::from_millis(ms).min(c.deadline.saturating_sub(c.clock.monotonic())),
            )?;
            if !r.is_empty() {
                self.log(format!(
                    "reply {}",
                    r.iter()
                        .take(32)
                        .map(|b| format!("{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
                if accept(&r) {
                    self.last_reply_accepted = true;
                    return Ok(Some(r));
                }
            }
        }
        Ok(None)
    }
    fn query(
        &mut self,
        s: &mut dyn HidSession,
        c: &PollContext<'_>,
        packet: &[u8],
        attempts: usize,
        ms: u64,
        accept: impl Fn(&[u8]) -> bool,
    ) -> Result<Option<Vec<u8>>, ProviderError> {
        if !c.active() {
            return Ok(None);
        }
        s.write(packet)?;
        self.receive(s, c, attempts, ms, accept)
    }
    fn razer_query(
        &mut self,
        s: &mut dyn HidSession,
        c: &PollContext<'_>,
        tid: u8,
        cmd: u8,
    ) -> Result<Option<u8>, ProviderError> {
        self.razer_status = 0;
        let request = p::razer_request(tid, cmd);
        s.send_feature(&request)?;
        let deadline = c.clock.monotonic() + Duration::from_secs(1);
        c.sleep(Duration::from_millis(60));
        let mut foreign = 0;
        loop {
            if !c.active() {
                break;
            }
            let response = s.feature(0, 91)?;
            let Some((status, value)) = p::razer_reply(&response, cmd) else {
                return Ok(None);
            };
            self.log(format!(
                "Razer tid={tid:02x} cmd=07:{cmd:02x} status={status:02x}"
            ));
            self.razer_status = u16::from(status);
            if status == 2 && value.is_some() {
                return Ok(value);
            }
            if status == 4 {
                return Ok(None);
            }
            if status != 1 && status != 2 {
                return Ok(None);
            }
            if c.clock.monotonic() >= deadline {
                break;
            }
            if status == 2 {
                foreign += 1;
                if foreign % 4 == 0
                    && foreign <= 8
                    && let Err(e) = s.send_feature(&request)
                {
                    if self.id != "wlmouse" {
                        return Err(e);
                    }
                    self.log(e.to_string());
                }
            }
            c.sleep(Duration::from_millis(80));
        }
        if foreign > 0 {
            self.razer_status = 0x100;
        }
        Ok(None)
    }
    fn pa_drain(&mut self, s: &mut dyn HidSession, c: &PollContext<'_>, barracuda: bool) {
        for _ in 0..if barracuda { 8 } else { 32 } {
            if !c.active() {
                break;
            }
            match s.read(64, Duration::from_millis(if barracuda { 20 } else { 5 })) {
                Ok(r) if !r.is_empty() => {}
                _ => break,
            }
        }
    }
    fn pa_remote(
        &mut self,
        s: &mut dyn HidSession,
        c: &PollContext<'_>,
        barracuda: bool,
        on: bool,
    ) -> bool {
        if barracuda {
            self.pa_drain(s, c, true)
        }
        if !c.active() {
            return false;
        }
        let result = s.write(&p::pa_request(barracuda, 0xe1, Some(on))).is_ok();
        if !barracuda {
            c.sleep(Duration::from_millis(35))
        }
        result
    }
    fn pa_query(
        &mut self,
        s: &mut dyn HidSession,
        c: &PollContext<'_>,
        barracuda: bool,
        command: u8,
    ) -> Result<Option<Vec<u8>>, ProviderError> {
        if !barracuda {
            self.pa_drain(s, c, false);
            self.pa_remote(s, c, false, true);
        }
        let mut result = None;
        for _ in 0..if barracuda { 4 } else { 3 } {
            if !c.active() {
                break;
            }
            if barracuda {
                self.pa_drain(s, c, true)
            } else {
                self.pa_remote(s, c, false, true);
            }
            if s.write(&p::pa_request(barracuda, command, None)).is_err() {
                break;
            }
            for _ in 0..if barracuda { 6 } else { 3 } {
                if !c.active() {
                    break;
                }
                if let Ok(reply) = s.read(
                    64,
                    Duration::from_millis(if barracuda { 150 } else { 50 })
                        .min(c.deadline.saturating_sub(c.clock.monotonic())),
                ) && let Some(payload) = p::pa_reply(&reply, barracuda, command)
                {
                    result = Some(payload.to_vec());
                    break;
                }
            }
            if result.is_some() {
                break;
            }
        }
        if !barracuda {
            self.pa_remote(s, c, false, false);
        }
        Ok(result)
    }
    fn read_razer(
        &mut self,
        d: &Device,
        info: &HidInfo,
        s: &mut dyn HidSession,
        c: &PollContext<'_>,
    ) -> Result<Option<Battery>, ProviderError> {
        let cache_key = format!("{:04x}:{}", d.pid, info.serial);
        let cached = self.razer_cache.get(&cache_key).cloned();
        let mut tids = vec![
            cached
                .as_ref()
                .filter(|(path, _)| path == &info.path)
                .map_or(d.parameter, |(_, tid)| *tid),
        ];
        for tid in [0x1f, 0x3f, 0xff, 0x9f, 8] {
            if !tids.contains(&tid) {
                tids.push(tid)
            }
        }
        for tid in tids {
            if !c.active() {
                break;
            }
            if let Some(raw) = self.razer_query(s, c, tid, 0x80)? {
                self.razer_cache
                    .insert(cache_key.clone(), (info.path.clone(), tid));
                let charging = self
                    .razer_query(s, c, tid, 0x84)?
                    .map_or(Some(false), |b| Some(b != 0));
                return Ok(p::battery(
                    (f64::from(raw) * 100.0 / 255.0).round_ties_even() as u8,
                    charging,
                ));
            }
            if [4, 0x100].contains(&self.razer_status) {
                self.razer_cache.insert(cache_key, (info.path.clone(), tid));
                break;
            }
        }
        Ok(None)
    }
    fn read(
        &mut self,
        d: &Device,
        info: &HidInfo,
        s: &mut dyn HidSession,
        c: &PollContext<'_>,
    ) -> Result<Option<Battery>, ProviderError> {
        match self.id {
            "razer" if [0x555, 0x556].contains(&d.pid) => self.read_pa(s, c, false),
            "razer" => self.read_razer(d, info, s, c),
            "barracuda" => self.read_pa(s, c, true),
            "wlmouse" if info.usage_page != 0xffff => {
                for _ in 0..8 {
                    if !c.active() {
                        break;
                    }
                    if let Ok(reply) = s.read(64, Duration::from_millis(40)) {
                        if reply.is_empty() {
                            break;
                        }
                        if let Some(b) = p::wl_heartbeat(&reply) {
                            return Ok(Some(b));
                        }
                    }
                }
                Ok(None)
            }
            "wlmouse" | "lamzu" | "gwolves" => {
                let request = p::padded(&[0, 0, 0, 2, 2, 0, 0x83], 65);
                s.send_feature(&request)?;
                for _ in 0..if self.id == "lamzu" { 10 } else { 15 } {
                    if !c.active() {
                        break;
                    }
                    c.sleep(Duration::from_millis(50));
                    for length in if self.id == "wlmouse" {
                        vec![65, 64]
                    } else {
                        vec![65]
                    } {
                        if let Ok(reply) = s.feature(0, length)
                            && let Some(b) = p::wl_feature(&reply)
                        {
                            return Ok(Some(b));
                        }
                    }
                }
                if self.id == "wlmouse" {
                    for _ in 0..4 {
                        let r = s.read(64, Duration::from_millis(50))?;
                        if let Some(b) = p::wl_heartbeat(&r) {
                            return Ok(Some(b));
                        }
                    }
                }
                Ok(None)
            }
            "asus" => {
                for _ in 0..16 {
                    if !c.active() {
                        return Ok(None);
                    }
                    match s.read(65, Duration::ZERO) {
                        Ok(reply) if !reply.is_empty() => {}
                        _ => break,
                    }
                }
                for _ in 0..3 {
                    if !c.active() {
                        break;
                    }
                    s.write(&p::padded(&[0, 0x12, 7], 65))?;
                    for _ in 0..8 {
                        if !c.active() {
                            break;
                        }
                        let reply = s.read(65, Duration::from_millis(300))?;
                        if reply.is_empty() {
                            break;
                        }
                        let shape = if reply.first() == Some(&0) {
                            &reply[1..]
                        } else {
                            &reply[..]
                        };
                        if shape.starts_with(&[0xff, 0xaa]) || reply.iter().all(|b| *b == 0) {
                            return Ok(None);
                        }
                        if shape.starts_with(&[0x12, 7]) {
                            return Ok(p::asus(&reply, d.parameter));
                        }
                    }
                }
                Ok(None)
            }
            "pulsar" => {
                for _ in 0..4 {
                    if !c.active() {
                        return Ok(None);
                    }
                    if s.read(17, Duration::from_millis(30))?.is_empty() {
                        break;
                    }
                }
                s.write(&p::pulsar_request())?;
                for _ in 0..4 {
                    if !c.active() {
                        break;
                    }
                    let reply = s.read(17, Duration::from_millis(250))?;
                    if reply.is_empty() {
                        break;
                    }
                    if let Some(b) = p::pulsar(&reply) {
                        return Ok(Some(b));
                    }
                    c.sleep(Duration::from_millis(20));
                }
                Ok(None)
            }
            "mchose" if d.vid == 0xa8a5 => {
                s.write(&p::padded(&[0, 0x55, 0x30, 0xa5, 0x0b, 0x2e, 1, 1, 1], 65))?;
                for _ in 0..25 {
                    if !c.active() {
                        break;
                    }
                    c.sleep(Duration::from_millis(20));
                    if let Ok(reply) = s.read(64, Duration::ZERO)
                        && let Some(b) = p::mchose_g7(&reply)
                    {
                        return Ok(Some(b));
                    }
                }
                Ok(None)
            }
            "mchose" => {
                for (id, length) in [(0x11, 20), (0x12, 64)] {
                    for _ in 0..4 {
                        if !c.active() {
                            break;
                        }
                        let mut packet = vec![255; length + 1];
                        packet[0] = id;
                        packet[1] = 6 ^ 255;
                        if s.send_feature(&packet).is_err() {
                            break;
                        }
                        c.sleep(Duration::from_millis(120));
                        if let Ok(reply) = s.feature(id, length + 1)
                            && let Some(b) = p::mchose(&reply)
                        {
                            self.mchose_models.insert(
                                d.vid,
                                u16::from_le_bytes([reply[4] ^ 255, reply[5] ^ 255]),
                            );
                            return Ok(Some(b));
                        }
                    }
                }
                Ok(None)
            }
            "astro" => {
                let r = self.query(
                    s,
                    c,
                    &p::padded(&[2, 0x0c, 3, 0, 6, 0x0c], 64),
                    8,
                    300,
                    |r| r.len() > 4 && r[4] == 6,
                )?;
                Ok(r.as_deref().and_then(p::astro))
            }
            "hyperx" => {
                s.write(&p::padded(&[6, 0xff, 0xbb, 2, 0], 52))?;
                c.sleep(Duration::from_millis(100));
                let reply = s.read(20, Duration::from_millis(1000))?;
                let mut battery = p::hyperx(&reply);
                if let Some(b) = &mut battery
                    && c.active()
                {
                    s.write(&p::padded(&[6, 0xff, 0xbb, 3, 0], 52))?;
                    c.sleep(Duration::from_millis(100));
                    let reply = s.read(20, Duration::from_millis(1000))?;
                    b.charging = if reply.len() > 4 && reply[..4] == [6, 0xff, 0xbb, 3] {
                        Some(reply[4] == 1)
                    } else {
                        Some(false)
                    };
                }
                Ok(battery)
            }
            "hyperx_cloud3" => {
                let mut out = None;
                for cmd in [0x89, 0x8a] {
                    let packet = p::padded(&[0x66, cmd], 62);
                    if let Err(e) = s.write(&packet) {
                        if e.message.to_lowercase().contains("incorrect function")
                            || e.message.contains("0x00000001")
                        {
                            s.send_feature(&packet)?
                        } else {
                            return Err(e);
                        }
                    }
                    c.sleep(Duration::from_millis(100));
                    let r = self.receive(s, c, 6, 200, |r| {
                        r.len() > 2
                            && r[0] == 0x66
                            && [cmd, if cmd == 0x89 { 0x0d } else { 0x0c }].contains(&r[1])
                    })?;
                    if cmd == 0x89 {
                        out = r.as_deref().and_then(p::hyperx3)
                    } else if let (Some(b), Some(r)) = (&mut out, r) {
                        b.charging = match r[2] {
                            0 => Some(false),
                            1 | 2 => Some(true),
                            _ => None,
                        }
                    }
                }
                Ok(out)
            }
            "hyperx_alpha2" => {
                for _ in 0..64 {
                    if !c.active() {
                        return Ok(None);
                    }
                    if s.read(64, Duration::from_millis(5))?.is_empty() {
                        break;
                    }
                }
                s.write(&p::padded(&[0x50, 2], 64))?;
                let deadline = (c.clock.monotonic() + Duration::from_millis(1500)).min(c.deadline);
                while c.active() && c.clock.monotonic() < deadline {
                    let reply = s.read(64, Duration::from_millis(150))?;
                    if let Some(b) = p::hyperx_alpha(&reply) {
                        return Ok(Some(b));
                    }
                    if reply.is_empty() {
                        c.sleep(Duration::from_millis(1));
                    }
                }
                Ok(None)
            }
            "keychron" => {
                for attempt in 0..3 {
                    if !c.active() {
                        break;
                    }
                    if attempt > 0 {
                        c.sleep(Duration::from_millis(100));
                    }
                    if s.send_feature(&p::padded(&[0xb3, 6], 64)).is_err() {
                        continue;
                    }
                    let reply = s.read(64, Duration::from_millis(500))?;
                    if let Some(b) = p::keychron(&reply) {
                        return Ok(Some(b));
                    }
                }
                Ok(None)
            }
            "jbl" => {
                for _ in 0..40 {
                    if !c.active() {
                        break;
                    }
                    let reply = s.read(64, Duration::from_millis(250))?;
                    if let Some(b) = p::jbl(&reply) {
                        return Ok(Some(b));
                    }
                    if !reply.is_empty() {
                        if reply.first() == Some(&9) {
                            self.log(format!("headset power state: {:?}", reply.get(1)));
                        }
                        c.sleep(Duration::from_millis(20));
                    }
                }
                Ok(None)
            }
            "am_infinity" => {
                for length in [65, 67] {
                    if !c.active() {
                        break;
                    }
                    if s.send_feature(&p::padded(&[0, 0xf7], length)).is_err() {
                        continue;
                    }
                    c.sleep(Duration::from_millis(30));
                    if let Ok(reply) = s.feature(5, 65)
                        && let Some(b) = p::am_infinity(&reply)
                    {
                        return Ok(Some(b));
                    }
                }
                Ok(None)
            }
            "corsair" if d.variant == "nxp" => {
                s.write(&p::padded(&[0, 0x0e, 0x50], 65))?;
                let reply = s.read(65, Duration::from_millis(500))?;
                Ok(p::corsair_nxp(&reply))
            }
            "corsair" => {
                s.write(&p::padded(&[0, 2, 8, 2, 0x13], 65))?;
                s.write(&p::padded(&[0, 2, 8, 2, 0x12], 65))?;
                for _ in 0..4 {
                    if s.read(64, Duration::from_millis(30))?.is_empty() {
                        break;
                    }
                }
                s.write(&p::padded(&[0, 2, 9, 2, 0x12], 65))?;
                if s.read(64, Duration::from_millis(500))?.is_empty() {
                    return Ok(None);
                }
                for _ in 0..4 {
                    if s.read(64, Duration::from_millis(30))?.is_empty() {
                        break;
                    }
                }
                for _ in 0..3 {
                    if !c.active() {
                        break;
                    }
                    s.write(&p::padded(&[0, 2, 9, 2, 0x0f], 65))?;
                    let reply = s.read(64, Duration::from_millis(500))?;
                    if let Some(b) = p::corsair(&reply) {
                        return Ok(Some(b));
                    }
                }
                Ok(None)
            }
            "audeze" => {
                let state_key = format!("{:04x}:{}", d.pid, info.serial);
                self.audeze_echo = false;
                let single = &[6, 7, 0x80, 5, 0x5a, 3, 0, 0xd6, 0x0c];
                let mut frames = Vec::new();
                let mut answered = true;
                let mut level = None;
                if !self.audeze_short_useless.contains(&state_key) {
                    let (got, active) = self.audeze_sequence(s, c, &[single])?;
                    answered = active;
                    level = got.iter().rev().find_map(|r| p::audeze(r));
                    frames.extend(got);
                }
                if level.is_none() && answered {
                    let (got, _) = self.audeze_sequence(s, c, AUDEZE_REQUESTS)?;
                    level = got.iter().rev().find_map(|r| p::audeze(r));
                    frames.extend(got);
                }
                if level.is_some() {
                    self.audeze_short_useless.remove(&state_key);
                } else {
                    self.audeze_short_useless.insert(state_key);
                }
                self.audeze_echo = frames.len() >= 5
                    && frames
                        .iter()
                        .all(|r| r.len() >= 4 && r[0] == 7 && r[3..].iter().all(|b| *b == 0));
                if self.audeze_echo {
                    self.log(
                        "empty echoes: headset did not answer; replug the dongle if persistent",
                    );
                }
                Ok(level)
            }
            "playstation" => {
                let bt = is_bluetooth(&info.path);
                if !bt || c.playstation_full_mode {
                    let _ = s.feature(if d.parameter == 1 { 5 } else { 2 }, 64);
                }
                let deadline = (c.clock.monotonic() + Duration::from_millis(1500)).min(c.deadline);
                while c.active() && c.clock.monotonic() < deadline {
                    let reply = match s.read(78, Duration::ZERO) {
                        Ok(r) => r,
                        Err(_) => break,
                    };
                    if reply.is_empty() {
                        c.sleep(Duration::from_millis(5));
                        continue;
                    }
                    if bt && !c.playstation_full_mode && reply[0] == 1 {
                        self.log("basic Bluetooth mode: no battery report; full mode not enabled");
                        return Ok(None);
                    }
                    let (rid, offset) = if bt {
                        if d.parameter == 1 {
                            (0x31, 54)
                        } else {
                            (0x11, 32)
                        }
                    } else {
                        (1, if d.parameter == 1 { 53 } else { 30 })
                    };
                    if reply[0] != rid || reply.len() <= offset {
                        continue;
                    }
                    if d.pid == 0x0ba0 && reply.len() > 31 && reply[31] & 4 != 0 {
                        return Ok(None);
                    }
                    return Ok(Some(if d.parameter == 1 {
                        p::dualsense(reply[offset])
                    } else {
                        p::ds4(reply[offset])
                    }));
                }
                Ok(None)
            }
            "eightbitdo" => {
                let rid = if is_bluetooth(&info.path) { 1 } else { 4 };
                let deadline = (c.clock.monotonic() + Duration::from_millis(400)).min(c.deadline);
                let mut reports = 0;
                while c.active() && c.clock.monotonic() < deadline && reports < 16 {
                    let reply = match s.read(64, Duration::ZERO) {
                        Ok(r) => r,
                        Err(_) => break,
                    };
                    if reply.is_empty() {
                        c.sleep(Duration::from_millis(5));
                        continue;
                    }
                    reports += 1;
                    if reply[0] == rid
                        && reply.len() > 14
                        && let Some(b) = p::eightbitdo(reply[14])
                    {
                        return Ok(Some(b));
                    }
                }
                Ok(None)
            }
            "nintendo" => {
                let start = c.clock.monotonic();
                let mut sent = Vec::new();
                while c.active() {
                    let now = c.clock.monotonic();
                    if sent.is_empty() && now - start >= Duration::from_millis(100)
                        || sent.len() == 1 && now - sent[0] >= Duration::from_millis(300)
                    {
                        let packet = p::padded(
                            &[1, self.counter & 15, 0, 1, 0x40, 0x40, 0, 1, 0x40, 0x40, 2],
                            49,
                        );
                        self.counter = self.counter.wrapping_add(1);
                        s.write(&packet)?;
                        sent.push(now);
                    }
                    if sent
                        .first()
                        .is_some_and(|first| now - *first >= Duration::from_millis(600))
                    {
                        break;
                    }
                    let reply = s.read(64, Duration::ZERO)?;
                    if reply.is_empty() {
                        c.sleep(Duration::from_millis(5));
                        continue;
                    }
                    if reply.len() >= 3 && [0x21, 0x30, 0x31].contains(&reply[0]) {
                        return Ok(p::nintendo(reply[2]));
                    }
                }
                Ok(None)
            }
            "lofree" => {
                let online = self.lofree_command(s, c, 0xaa)?;
                if online.is_none_or(|value| value == 0) {
                    return Ok(None);
                }
                let level = self.lofree_command(s, c, 0x1a)?;
                Ok(level.and_then(|v| p::battery(v, None)))
            }
            "steelseries" => self.steelseries(s, c, d),
            _ => Err(ProviderError::new("unregistered protocol")),
        }
    }
    fn audeze_sequence(
        &mut self,
        s: &mut dyn HidSession,
        c: &PollContext<'_>,
        requests: &[&[u8]],
    ) -> Result<(Vec<Vec<u8>>, bool), ProviderError> {
        let mut frames = Vec::new();
        let mut unanswered = 0;
        for packet in requests {
            if !c.active() {
                return Ok((frames, false));
            }
            if s.write(&p::padded(packet, 62)).is_err() {
                break;
            }
            c.sleep(Duration::from_millis(60));
            if let Ok(reply) = s.input_report(7, 62) {
                if !reply.is_empty() {
                    frames.push(reply);
                } else if frames.is_empty() {
                    unanswered += 1;
                }
            } else if frames.is_empty() {
                unanswered += 1;
            }
            if unanswered >= 3 {
                return Ok((frames, false));
            }
        }
        for _ in 0..2 {
            if !c.active() {
                break;
            }
            c.sleep(Duration::from_millis(60));
            if let Ok(reply) = s.input_report(7, 62)
                && !reply.is_empty()
            {
                frames.push(reply);
            }
        }
        Ok((frames, true))
    }
    fn read_pa(
        &mut self,
        s: &mut dyn HidSession,
        c: &PollContext<'_>,
        barracuda: bool,
    ) -> Result<Option<Battery>, ProviderError> {
        self.pa_accepted = false;
        let mut woke = false;
        for attempt in 0..if barracuda { 3 } else { 4 } {
            if !c.active() {
                return Ok(None);
            }
            if self.pa_remote(s, c, barracuda, true) {
                woke = true;
                break;
            }
            c.sleep(Duration::from_millis(150 * (attempt + 1)));
        }
        if !woke {
            return if barracuda {
                Ok(None)
            } else {
                Err(ProviderError::new("PA receiver needs reopen"))
            };
        }
        self.pa_accepted = true;
        if barracuda {
            c.sleep(Duration::from_millis(50));
        }
        let result = (|| {
            let level = self
                .pa_query(s, c, barracuda, 0x21)?
                .and_then(|r| r.first().copied());
            let Some(level) = level else { return Ok(None) };
            let charging = self
                .pa_query(s, c, barracuda, 0x2a)?
                .and_then(|r| r.first().copied())
                .is_some_and(|b| b != 0);
            Ok(p::battery(
                if barracuda { level } else { level.min(100) },
                Some(charging),
            ))
        })();
        if barracuda {
            self.pa_remote(s, c, true, false);
        }
        result
    }
    fn lofree_wait(
        &mut self,
        s: &mut dyn HidSession,
        c: &PollContext<'_>,
        deadline: Duration,
        accept: impl Fn(&[u8]) -> bool,
    ) -> Option<Vec<u8>> {
        while c.active() && c.clock.monotonic() < deadline {
            let before = c.clock.monotonic();
            let reply = match s.read(
                64,
                Duration::from_millis(100).min(deadline.saturating_sub(before)),
            ) {
                Ok(r) => r,
                Err(e) => {
                    self.log(e.to_string());
                    return None;
                }
            };
            if accept(&reply) {
                return Some(reply);
            }
            if reply.is_empty() && c.clock.monotonic() == before {
                c.sleep(Duration::from_millis(100).min(deadline.saturating_sub(before)));
            }
        }
        None
    }
    fn lofree_command(
        &mut self,
        s: &mut dyn HidSession,
        c: &PollContext<'_>,
        cmd: u8,
    ) -> Result<Option<u8>, ProviderError> {
        let deadline = (c.clock.monotonic() + Duration::from_secs(3)).min(c.deadline);
        s.write(&p::padded(&[4, 0, 0, 1], 32))?;
        if self
            .lofree_wait(s, c, deadline, |r| r.len() > 3 && r[0] == 4 && r[3] == 1)
            .is_none()
        {
            return Ok(None);
        }
        s.write(&p::padded(&[4, 0, 0, cmd, 0, 0, 0, 0], 32))?;
        let reply = self.lofree_wait(s, c, deadline, |r| {
            r.len() > 8 && r[0] == 4 && [0, cmd].contains(&r[3]) && r[5] == 0 && r[6] == 0
        });
        let _ = s.write(&p::padded(&[4, 0, 0, 2], 32));
        let stop_deadline = deadline.min(c.clock.monotonic() + Duration::from_millis(500));
        let _ = self.lofree_wait(s, c, stop_deadline, |r| {
            r.len() > 3 && r[0] == 4 && r[3] == 2
        });
        Ok(reply.map(|r| r[8]))
    }
    fn steelseries(
        &mut self,
        s: &mut dyn HidSession,
        c: &PollContext<'_>,
        d: &Device,
    ) -> Result<Option<Battery>, ProviderError> {
        let r = match d.variant {
            "exchange_arctis1" => self
                .query(s, c, &[6, 0x12], 10, 100, |r| {
                    r.len() >= 4 && r[..2] == [6, 0x12]
                })?
                .and_then(|r| {
                    if r[2] == 1 {
                        None
                    } else {
                        p::battery(r[3].min(100), None)
                    }
                }),
            "exchange_arctis7_2018" => {
                let link = self.query(s, c, &[6, 0x14], 10, 100, |r| {
                    r.len() >= 4 && r[..2] == [6, 0x14]
                })?;
                if link.is_none_or(|r| r[2] != 3) {
                    return Ok(None);
                }
                self.query(s, c, &[6, 0x18], 10, 100, |r| {
                    r.len() >= 4 && r[..2] == [6, 0x18]
                })?
                .and_then(|r| p::battery(r[2].min(100), None))
            }
            "exchange_arctis7" => self
                .query(s, c, &[6, 0x18], 10, 100, |r| {
                    r.len() >= 4 && r[..2] == [6, 0x18]
                })?
                .and_then(|r| {
                    if r[2] == 0 {
                        None
                    } else {
                        p::battery(r[2].min(100), None)
                    }
                }),
            "exchange_arctis9" => self
                .query(s, c, &[0, 0x20], 10, 100, |r| {
                    r.len() >= 5 && [0xaa, 0x55].contains(&r[0])
                })?
                .and_then(|r| {
                    if r[0] != 0xaa || r[1] != 1 {
                        None
                    } else {
                        p::battery(
                            ((u16::from(r[3].clamp(0x64, 0x9a)) - 0x64) * 100 / (0x9a - 0x64))
                                as u8,
                            Some(r[4] == 1),
                        )
                    }
                }),
            "exchange_pro_wireless" => {
                let state = self.query(s, c, &p::padded(&[0x41, 0xaa], 31), 10, 100, |r| {
                    !r.is_empty() && [2, 4].contains(&r[0])
                })?;
                if state.is_none_or(|r| r[0] == 2) {
                    return Ok(None);
                }
                self.query(s, c, &p::padded(&[0x40, 0xaa], 31), 10, 100, |r| {
                    !r.is_empty() && r[0] <= 4
                })?
                .and_then(|r| {
                    let mut b = p::battery(r[0] * 25, None)?;
                    b.coarse = true;
                    Some(b)
                })
            }
            "exchange_nova_pro" => self
                .query(s, c, &[6, 0xb0], 10, 100, |r| {
                    r.len() >= 16 && r[0] == 0xb0 && [1, 2, 8].contains(&r[15])
                })?
                .and_then(|r| {
                    if r[15] == 1 || r[6] > 8 {
                        None
                    } else {
                        let mut b =
                            p::battery((u16::from(r[6]) * 100 / 8) as u8, Some(r[15] == 2))?;
                        b.coarse = true;
                        Some(b)
                    }
                }),
            variant if ["parse_rival3", "parse_aerox3"].contains(&variant) => {
                let (request, echo) = if variant == "parse_aerox3" {
                    (p::padded(&[0, 0xd2], 64), 0xd2)
                } else {
                    (p::padded(&[0, 0xaa, 1], 64), 0xaa)
                };
                for _ in 0..3 {
                    if !c.active() {
                        break;
                    }
                    if s.write(&request).is_err() {
                        continue;
                    }
                    for _ in 0..6 {
                        if !c.active() {
                            break;
                        }
                        let reply = s.read(64, Duration::from_millis(100))?;
                        if reply.is_empty() {
                            continue;
                        }
                        if reply[0] == echo
                            || reply.first() == Some(&0) && reply.get(1) == Some(&echo)
                        {
                            return Ok(p::steelseries(&reply, variant));
                        }
                        if let Some(b) = p::steelseries(&reply, variant) {
                            return Ok(Some(b));
                        }
                        break;
                    }
                }
                None
            }
            variant => self
                .query(s, c, &[0, 0xb0], 10, 100, |r| r.first() == Some(&0xb0))?
                .as_deref()
                .and_then(|r| p::steelseries(r, variant)),
        };
        Ok(r)
    }
    // HID++ addresses and two response channels are distinct protocol inputs.
    #[allow(clippy::too_many_arguments)]
    fn logitech_request(
        &mut self,
        long: &mut dyn HidSession,
        short: &mut Option<Box<dyn HidSession>>,
        c: &PollContext<'_>,
        slot: u8,
        feature: u8,
        function: u8,
        params: &[u8],
    ) -> Result<Option<Vec<u8>>, ProviderError> {
        self.logitech_error = None;
        if !c.active() {
            return Ok(None);
        }
        self.counter = self.counter.wrapping_add(1);
        let token = (function << 4) | (10 + self.counter % 6);
        let mut packet = p::padded(&[0x11, slot, feature, token], 20);
        packet[4..4 + params.len()].copy_from_slice(params);
        long.write(&packet)?;
        let deadline =
            (c.clock.monotonic() + Duration::from_millis(self.logitech_timeout)).min(c.deadline);
        while c.active() && c.clock.monotonic() < deadline {
            for channel in 0..2 {
                let r = if channel == 0 {
                    long.read(64, Duration::ZERO)?
                } else if let Some(s) = short {
                    s.read(64, Duration::ZERO)?
                } else {
                    continue;
                };
                if r.len() < 4 || r[1] != slot {
                    continue;
                }
                if [0x8f, 0xff].contains(&r[2]) && r.len() >= 6 && r[3] == feature && r[4] == token
                {
                    self.logitech_error = Some(r[5]);
                    return Ok(None);
                }
                if r[2] == feature && r[3] == token {
                    let mut payload = r[4..].to_vec();
                    payload.resize(payload.len().max(16), 0);
                    return Ok(Some(payload));
                }
            }
            c.sleep(Duration::from_millis(5));
        }
        Ok(None)
    }
    fn logitech_identity_read(
        &mut self,
        long: &mut dyn HidSession,
        short: &mut Option<Box<dyn HidSession>>,
        c: &PollContext<'_>,
        slot: u8,
    ) -> Result<(String, String, String), ProviderError> {
        let (mut name, mut kind, mut unit) = (String::new(), String::new(), String::new());
        if let Some(feature) = self
            .logitech_request(long, short, c, slot, 0, 0, &[0, 5])?
            .map(|r| r[0])
            .filter(|f| *f != 0)
        {
            let length = self
                .logitech_request(long, short, c, slot, feature, 0, &[])?
                .map_or(0, |r| r[0]);
            let mut bytes = Vec::new();
            while bytes.len() < usize::from(length) && c.active() {
                let Some(chars) =
                    self.logitech_request(long, short, c, slot, feature, 1, &[bytes.len() as u8])?
                else {
                    break;
                };
                bytes.extend_from_slice(&chars[..16]);
            }
            name = String::from_utf8_lossy(&bytes[..bytes.len().min(length as usize)])
                .trim()
                .into();
            if let Some(r) = self.logitech_request(long, short, c, slot, feature, 2, &[])? {
                kind = match r[0] {
                    0 | 2 => "keyboard",
                    3..=5 => "mouse",
                    _ => "",
                }
                .into();
            }
        }
        if let Some(feature) = self
            .logitech_request(long, short, c, slot, 0, 0, &[0, 3])?
            .map(|r| r[0])
            .filter(|f| *f != 0)
            && let Some(r) = self.logitech_request(long, short, c, slot, feature, 0, &[])?
            && r[1..5].iter().any(|b| *b != 0)
        {
            unit = r[1..5].iter().map(|b| format!("{b:02X}")).collect();
        }
        Ok((name, kind, unit))
    }
    fn logitech_poll(
        &mut self,
        infos: &[HidInfo],
        hid: &dyn HidTransport,
        c: &PollContext<'_>,
    ) -> PollResult {
        let mut groups: BTreeMap<(u16, String), BTreeMap<u8, &HidInfo>> = BTreeMap::new();
        let mut headsets = BTreeMap::new();
        for d in infos {
            let key = (d.product_id, receiver_key(d));
            if d.usage_page == 0xff00 && [1, 2].contains(&d.usage) {
                groups.entry(key).or_default().insert(d.usage as u8, d);
            } else if DEVICES.iter().any(|known| {
                known.provider == "logitech"
                    && known.pid == d.product_id
                    && known.variant == "headset"
            }) && (d.usage_page, d.usage)
                == if d.product_id == 0x0ac4 {
                    (0x0c, 1)
                } else {
                    (0xff43, 0x202)
                }
            {
                headsets.insert(key, d);
            }
        }
        for (key, info) in headsets {
            groups.entry(key).or_default().entry(2).or_insert(info);
        }
        let mut found: BTreeMap<String, Reading> = BTreeMap::new();
        for ((pid, group), paths) in &groups {
            if !c.active() {
                break;
            }
            let Some(long_info) = paths.get(&2) else {
                continue;
            };
            let mut long = match hid.open(long_info) {
                Ok(s) => s,
                Err(e) => {
                    self.log(format!("open: {e}"));
                    continue;
                }
            };
            let mut short = paths.get(&1).and_then(|i| hid.open(i).ok());
            self.counter = 0;
            let receiver = DEVICES
                .iter()
                .any(|d| d.provider == "logitech" && d.pid == *pid && d.variant == "receiver")
                || long_info.product.to_lowercase().contains("receiver");
            let multi = groups.keys().filter(|(other, _)| other == pid).count() > 1;
            let result: Result<(), ProviderError> = (|| {
                for slot in if receiver {
                    vec![1, 2, 3, 4, 5, 6]
                } else {
                    vec![255]
                } {
                    if !c.active() {
                        break;
                    }
                    let slot_key = format!("{pid:04x}:{group}:{slot}");
                    self.logitech_timeout = if self.logitech_asleep.contains(&slot_key) {
                        600
                    } else {
                        2000
                    };
                    let ping = self.logitech_request(&mut *long, &mut short, c, slot, 0, 1, &[])?;
                    self.logitech_timeout = 600;
                    if ping.is_none() {
                        if let Some(error) = self.logitech_error {
                            self.logitech_asleep.remove(&slot_key);
                            if error == 8 {
                                self.logitech_identity.remove(&slot_key);
                            }
                        } else {
                            self.logitech_asleep.insert(slot_key);
                        }
                        continue;
                    }
                    self.logitech_asleep.remove(&slot_key);
                    let (name, mut kind, unit) =
                        if let Some(identity) = self.logitech_identity.get(&slot_key) {
                            identity.clone()
                        } else {
                            let identity =
                                self.logitech_identity_read(&mut *long, &mut short, c, slot)?;
                            if !identity.0.is_empty() || !identity.2.is_empty() {
                                self.logitech_identity
                                    .insert(slot_key.clone(), identity.clone());
                            }
                            identity
                        };
                    let headset = DEVICES.iter().find(|d| {
                        d.provider == "logitech" && d.pid == *pid && d.variant == "headset"
                    });
                    if headset.is_some() {
                        kind = "headset".into();
                    }
                    let name = if name.is_empty() {
                        headset.map_or("Logitech device", |d| d.name).into()
                    } else {
                        name
                    };
                    for feature in [0x1004u16, 0x1000, 0x1001, 0x1f20] {
                        let index = self
                            .logitech_request(
                                &mut *long,
                                &mut short,
                                c,
                                slot,
                                0,
                                0,
                                &feature.to_be_bytes(),
                            )?
                            .map_or(0, |r| r[0]);
                        if index == 0 {
                            continue;
                        }
                        let params = self.logitech_request(
                            &mut *long,
                            &mut short,
                            c,
                            slot,
                            index,
                            u8::from(feature == 0x1004),
                            &[],
                        )?;
                        let Some(battery) = params.as_deref().and_then(|r| p::logitech(feature, r))
                        else {
                            continue;
                        };
                        let prefix = if multi {
                            format!("{group}:")
                        } else {
                            String::new()
                        };
                        let key = if unit.is_empty() {
                            format!("logitech:{prefix}{pid:04x}:{slot}")
                        } else {
                            format!("logitech:{prefix}{unit}")
                        };
                        if let Some(old) = self.logitech_slots.insert(slot_key.clone(), key.clone())
                            && old != key
                        {
                            self.last.remove(&old);
                        }
                        let mut reading = Reading::new(&key, &name, "logitech", c.clock.unix());
                        reading.kind = kind.clone();
                        reading.serial = (!unit.is_empty()).then(|| unit.clone());
                        reading.container = long_info.container.clone();
                        set_battery(&mut reading, battery);
                        if !found.contains_key(&key) || reading.charging == Some(true) {
                            found.insert(key, reading);
                        }
                        break;
                    }
                }
                Ok(())
            })();
            if let Err(e) = result {
                self.log(format!("channel error: {e}"));
            }
            self.logitech_timeout = 600;
        }
        for (key, reading) in &found {
            self.last.insert(key.clone(), reading.clone());
        }
        let mut out: Vec<_> = found.values().cloned().collect();
        let now = c.clock.unix();
        self.last.retain(|key, reading| {
            if found.contains_key(key) {
                true
            } else if !groups.is_empty() && now - reading.timestamp < 300 {
                let mut stale = reading.clone();
                stale.connection = Connection::Sleeping;
                out.push(stale);
                true
            } else {
                false
            }
        });
        Ok(out)
    }
    fn cached(&self, key: &str, now: i64) -> Option<Reading> {
        let r = self.last.get(key)?;
        if ![
            "razer",
            "wlmouse",
            "mchose",
            "gwolves",
            "lamzu",
            "am_infinity",
            "nintendo",
            "jbl",
            "logitech",
        ]
        .contains(&self.id)
        {
            return None;
        }
        if self.id == "razer"
            && (r.kind == "headset" || r.name.to_lowercase().contains("blackshark"))
        {
            return None;
        }
        if self.id != "jbl" && now - r.timestamp >= 300 {
            return None;
        }
        let mut r = r.clone();
        r.connection = Connection::Sleeping;
        Some(r)
    }
}
fn set_battery(r: &mut Reading, b: Battery) {
    r.level = Some(b.level);
    r.charging = b.charging.or(Some(false));
    if b.coarse {
        r.precision = Precision::Coarse;
        r.approx = b.label.or_else(|| Some(format!("about {}%", b.level)));
    }
}
pub fn is_bluetooth(path: &str) -> bool {
    let p = path.to_lowercase();
    p.contains("vid&") || p.contains("00001124-0000-1000-8000-00805f9b34fb")
}
pub fn receiver_key(info: &HidInfo) -> String {
    info.container.clone().unwrap_or_else(|| {
        let segment = info.path.split('#').nth(2).unwrap_or(&info.path);
        segment
            .rsplit_once('&')
            .map_or(segment, |(prefix, _)| prefix)
            .to_lowercase()
    })
}
fn candidate(id: &str, d: &Device, i: &HidInfo) -> bool {
    let usage = (i.usage_page, i.usage);
    match id {
        "razer" => d.pid != 0x053a,
        "barracuda" => i.usage_page == 0xff00,
        "audeze" => usage == (0xff13, 1),
        "hyperx" => usage == (0xff90, 0x0303),
        "hyperx_cloud3" | "jbl" => usage == (0xff13, 1),
        "hyperx_alpha2" => usage == (0xff13, 0xff00),
        "astro" => usage == (0xff32, 0x74),
        "lamzu" => i.interface == 2 && i.usage_page == 0xffff,
        "gwolves" => i.feature_length == Some(65),
        "lofree" => usage == (0xff1c, 0x92),
        "am_infinity" => usage == (0xffff, 2),
        "asus" => i.interface == 0 && i.usage_page >= 0xff00,
        "mchose" => {
            if d.vid == 0xa8a5 {
                i.usage_page == 0xff01
            } else {
                i.usage_page >= 0xff00
            }
        }
        "keychron" => i.interface == 4,
        "pulsar" => usage == (0xff02, 2) || (i.interface == 1 && i.usage_page >= 0xff00),
        "corsair" => {
            if d.variant == "nxp" {
                i.usage_page == 0xff42
            } else {
                i.interface == 4 && i.usage_page >= 0xff00
            }
        }
        "playstation" => true,
        "eightbitdo" => true,
        "nintendo" => is_bluetooth(&i.path),
        "steelseries" => match d.variant {
            "exchange_arctis1" => i.interface == 3 && i.usage_page == 0xff43,
            "exchange_arctis7" | "exchange_arctis7_2018" => {
                i.interface == 5 && i.usage_page >= 0xff00
            }
            "exchange_arctis9" | "exchange_pro_wireless" => {
                i.interface == 0 && i.usage_page >= 0xff00
            }
            "exchange_nova_pro" => [3, 4].contains(&i.interface) && i.usage_page >= 0xff00,
            _ => usage == (0xffc0, 1),
        },
        _ => i.usage_page >= 0xff00,
    }
}
fn candidate_group(id: &str, d: &Device, i: &HidInfo, infos: &[HidInfo]) -> bool {
    let mine: Vec<_> = infos
        .iter()
        .filter(|other| other.vendor_id == i.vendor_id && other.product_id == i.product_id)
        .collect();
    match id {
        "astro" | "keychron" | "jbl" => {
            let picked = mine
                .iter()
                .find(|other| candidate(id, d, other))
                .or_else(|| mine.first());
            picked.is_some_and(|picked| picked.path == i.path)
        }
        "corsair" if d.variant == "nxp" => {
            if mine.iter().any(|other| other.usage_page == 0xff42) {
                i.usage_page == 0xff42
            } else {
                true
            }
        }
        "corsair" => mine
            .iter()
            .find(|other| other.interface == 4)
            .or_else(|| mine.first())
            .is_some_and(|picked| picked.path == i.path),
        "pulsar" => {
            let control: Vec<_> = mine
                .iter()
                .copied()
                .filter(|other| (other.usage_page, other.usage) == (0xff02, 2))
                .collect();
            let picked = control
                .iter()
                .chain(mine.iter())
                .find(|other| other.output_length.is_none_or(|n| n == 17))
                .or_else(|| control.first())
                .or_else(|| mine.first());
            picked.is_some_and(|picked| picked.path == i.path)
        }
        "audeze" => {
            let preferred = mine
                .iter()
                .any(|other| (other.usage_page, other.usage) == (0xff13, 1));
            let page = mine.iter().any(|other| other.usage_page == 0xff13);
            if preferred {
                (i.usage_page, i.usage) == (0xff13, 1)
            } else if page {
                i.usage_page == 0xff13
            } else {
                i.usage_page >= 0xff00
            }
        }
        "steelseries" if !d.variant.starts_with("exchange_") => mine
            .iter()
            .filter(|other| (other.usage_page, other.usage) == (0xffc0, 1))
            .min_by_key(|other| other.interface != 3)
            .is_some_and(|picked| picked.path == i.path),
        _ => candidate(id, d, i),
    }
}
impl BatteryProvider for HidProvider {
    fn id(&self) -> &'static str {
        self.id
    }
    fn diagnostics(&self) -> Vec<String> {
        self.diagnostics.clone()
    }
    fn poll(&mut self, hid: &dyn HidTransport, c: &PollContext<'_>) -> PollResult {
        self.diagnostics.clear();
        let vendors: BTreeSet<_> = DEVICES
            .iter()
            .filter(|d| d.provider == self.id)
            .map(|d| d.vid)
            .collect();
        let mut infos = Vec::new();
        for v in vendors {
            infos.extend(hid.enumerate(v)?);
        }
        if self.id == "logitech" {
            return self.logitech_poll(&infos, hid, c);
        }
        let mut selected: Vec<_> = infos
            .iter()
            .filter_map(|i| {
                DEVICES
                    .iter()
                    .find(|d| {
                        d.provider == self.id && d.vid == i.vendor_id && d.pid == i.product_id
                    })
                    .copied()
                    .or_else(|| {
                        if self.id == "mchose" && [0x5253, 0x3837].contains(&i.vendor_id) {
                            Some(Device {
                                provider: "mchose",
                                vid: i.vendor_id,
                                pid: i.product_id,
                                name: "MCHOSE mouse",
                                variant: "",
                                parameter: 0,
                            })
                        } else {
                            None
                        }
                    })
                    .map(|d| (d, i))
            })
            .filter(|(d, i)| candidate_group(self.id, d, i, &infos))
            .collect();
        if self.id == "steelseries" {
            selected.retain(|(d, i)| {
                !d.variant.starts_with("exchange_")
                    || self.steel_path.get(&d.pid).is_none_or(|path| {
                        !infos.iter().any(|other| &other.path == path) || &i.path == path
                    })
            });
        }
        selected.sort_by_key(|(_, i)| {
            (
                !([0x4b1a, 0x4b1e, 0x001c].contains(&i.product_id)
                    || self.id == "gwolves" && i.product_id != 0x3854),
                if self.id == "razer" {
                    let key = format!("{:04x}:{}", i.product_id, i.serial);
                    if self
                        .razer_cache
                        .get(&key)
                        .is_some_and(|(path, _)| path == &i.path)
                    {
                        0
                    } else if [1, 0xff00].contains(&i.usage_page) {
                        1
                    } else {
                        2
                    }
                } else if self.id == "corsair" {
                    i32::from(i.usage != 1)
                } else if self.id == "wlmouse" {
                    if i.usage_page == 0xffff { 0 } else { 1 }
                } else if self.id == "mchose" {
                    i32::from(i.usage_page != 0xff01)
                } else if ["playstation", "eightbitdo"].contains(&self.id) {
                    i32::from(!(i.usage_page == 1 && [4, 5].contains(&i.usage)))
                } else {
                    0
                },
                if i.interface < 0 { 99 } else { i.interface },
                i.path.clone(),
            )
        });
        let mut output = Vec::new();
        let mut seen = BTreeSet::new();
        let mut errors = Vec::new();
        for (d, i) in selected {
            if !c.active() {
                break;
            }
            if ["wlmouse", "gwolves", "lamzu"].contains(&self.id)
                && output.iter().any(|r: &Reading| r.charging == Some(true))
            {
                break;
            }
            if self.id == "audeze"
                && ![0x4b1a, 0x4b1e].contains(&d.pid)
                && i.product.eq_ignore_ascii_case("Audeze Maxwell Dongle")
            {
                continue;
            }
            if self.id == "lofree" && infos.iter().any(|i| i.product_id == 0x24) {
                continue;
            }
            let identity = if !i.serial.is_empty() && !i.serial.chars().all(|c| c == '0') {
                i.serial.clone()
            } else {
                receiver_key(i)
            };
            let key = if ["wlmouse", "gwolves", "lamzu", "am_infinity", "lofree"].contains(&self.id)
            {
                self.id.into()
            } else if self.id == "mchose" {
                if d.vid == 0x5253 {
                    "mchose".into()
                } else {
                    format!("mchose:{:04x}", d.vid)
                }
            } else if self.id == "asus" {
                format!("asus:{}", d.name.to_lowercase().replace(' ', "-"))
            } else if self.id == "audeze" {
                format!("audeze:{}", i.serial)
            } else if self.id == "playstation" {
                format!("ps:{:04x}:{identity}", d.pid)
            } else if self.id == "eightbitdo" {
                format!("8bitdo:{:04x}:{}", d.pid, i.serial)
            } else if self.id == "razer" {
                format!(
                    "razer:{:04x}:{}",
                    d.pid,
                    if i.serial.is_empty() {
                        let named: BTreeSet<_> = infos
                            .iter()
                            .filter(|other| other.product_id == d.pid && !other.serial.is_empty())
                            .map(|other| other.serial.clone())
                            .collect();
                        if named.len() == 1 {
                            named.into_iter().next().unwrap()
                        } else {
                            String::new()
                        }
                    } else {
                        i.serial.clone()
                    }
                )
            } else if self.id == "nintendo" {
                format!("switch:{:04x}:{}", d.pid, i.serial)
            } else if ["hyperx_cloud3", "hyperx_alpha2"].contains(&self.id) {
                format!("hyperx:{:04x}", d.pid)
            } else {
                format!("{}:{:04x}", self.id, d.pid)
            };
            if !["mchose", "wlmouse", "gwolves", "lamzu"].contains(&self.id) && seen.contains(&key)
            {
                continue;
            }
            self.log(format!(
                "{} {:04x}:{:04x} interface {} usage {:04x}:{:04x}",
                d.name, i.vendor_id, i.product_id, i.interface, i.usage_page, i.usage
            ));
            if self.id == "razer"
                && self
                    .razer_dead
                    .get(&i.path)
                    .is_some_and(|until| *until > c.clock.unix())
            {
                continue;
            }
            let mut session = match hid.open(i) {
                Ok(s) => s,
                Err(e) => {
                    self.log(e.to_string());
                    if self.id == "razer" && ![0x555, 0x556].contains(&d.pid) {
                        self.razer_dead.insert(i.path.clone(), c.clock.unix() + 300);
                    }
                    if ["playstation", "eightbitdo"].contains(&self.id) {
                        let mut reading = Reading::new(&key, d.name, self.id, c.clock.unix());
                        reading.kind = "gamepad".into();
                        reading.charging = Some(false);
                        seen.insert(key);
                        output.push(reading);
                    } else {
                        errors.push(e);
                    }
                    continue;
                }
            };
            let razer_cached = self.id == "razer"
                && self
                    .razer_cache
                    .get(&format!("{:04x}:{}", d.pid, i.serial))
                    .is_some_and(|(path, _)| path == &i.path);
            self.last_reply_accepted = false;
            let mut result = self.read(&d, i, &mut *session, c);
            if self.id == "steelseries"
                && d.variant.starts_with("exchange_")
                && self.last_reply_accepted
            {
                self.steel_path.insert(d.pid, i.path.clone());
                seen.insert(key.clone());
            }
            if self.id == "razer"
                && ![0x555, 0x556].contains(&d.pid)
                && d.name.to_lowercase().contains("blackshark")
                && result.as_ref().is_ok_and(|r| r.is_none())
            {
                result = self.read_pa(&mut *session, c, false);
            }
            if self.id == "razer"
                && (self.razer_status == 0x100 || razer_cached && self.razer_status == 4)
            {
                seen.insert(key.clone());
            }
            if result
                .as_ref()
                .is_err_and(|e| e.message == "PA receiver needs reopen")
                && c.active()
            {
                drop(session);
                c.sleep(Duration::from_millis(300));
                result = match hid.open(i) {
                    Ok(mut reopened) => self.read(&d, i, &mut *reopened, c),
                    Err(e) => Err(e),
                };
            }
            if self.id == "razer" && [0x555, 0x556].contains(&d.pid) && self.pa_accepted {
                self.razer_cache
                    .insert(format!("{:04x}:{}", d.pid, i.serial), (i.path.clone(), 0));
                if result.as_ref().is_ok_and(|r| r.is_none()) {
                    seen.insert(key.clone());
                }
            }
            if self.id == "razer" && ![0x555, 0x556].contains(&d.pid) && result.is_err() {
                self.razer_dead.insert(i.path.clone(), c.clock.unix() + 300);
            }
            match result {
                Ok(Some(b)) => {
                    let mut r = Reading::new(&key, d.name, self.id, c.clock.unix());
                    if d.name == "Razer wireless device" && !i.product.trim().is_empty() {
                        r.name = i.product.trim().into();
                    }
                    r.serial = (!i.serial.is_empty()).then(|| i.serial.clone());
                    r.container = i.container.clone();
                    r.kind = if ["playstation", "eightbitdo", "nintendo"].contains(&self.id) {
                        "gamepad"
                    } else if self.id == "lofree"
                        || self.id == "razer" && [0x25a, 0x25c].contains(&d.pid)
                    {
                        "keyboard"
                    } else if [
                        "audeze",
                        "barracuda",
                        "astro",
                        "hyperx",
                        "hyperx_cloud3",
                        "hyperx_alpha2",
                        "jbl",
                        "corsair",
                        "steelseries",
                    ]
                    .contains(&self.id)
                        && !(d.variant == "nxp"
                            || d.variant.contains("rival")
                            || d.variant.contains("aerox"))
                    {
                        "headset"
                    } else {
                        "mouse"
                    }
                    .into();
                    r.via = if is_bluetooth(&i.path) {
                        "bluetooth"
                    } else {
                        "usb"
                    }
                    .into();
                    set_battery(&mut r, b);
                    if self.id == "audeze" {
                        self.stuck.remove(&key);
                        r.charging = Some([0x4b1a, 0x4b1e].contains(&d.pid));
                        r.charging_inferred = true;
                        let first = *self.first_seen.entry(key.clone()).or_insert(c.clock.unix());
                        if r.level == Some(0) && c.clock.unix() - first < 90 {
                            r.level = None;
                        }
                    }
                    if ["mchose", "wlmouse", "gwolves", "lamzu"].contains(&self.id) {
                        if self.id == "mchose" {
                            r.name = if self.mchose_models.get(&d.vid) == Some(&0x31) {
                                "MCHOSE M7 Ultra".into()
                            } else if !i.product.trim().is_empty() {
                                i.product.trim().into()
                            } else if let Some(model) = self.mchose_models.get(&d.vid) {
                                format!("MCHOSE mouse (0x{model:04x})")
                            } else {
                                d.name.into()
                            };
                        } else {
                            let generic = d.name.to_lowercase().contains("receiver")
                                || ["WLmouse", "G-Wolves mouse", "LAMZU Maya X"].contains(&d.name)
                                    && self.family_names.contains_key(self.id);
                            if !generic {
                                self.family_names.insert(self.id.into(), r.name.clone());
                            }
                            if let Some(name) = self.family_names.get(self.id) {
                                r.name = name.clone();
                            }
                        }
                        if let Some(existing) =
                            output.iter().position(|old: &Reading| old.key == key)
                        {
                            if r.charging == Some(true) {
                                output[existing] = r.clone();
                                self.last.insert(key.clone(), r);
                            }
                            seen.insert(key);
                            continue;
                        }
                    }
                    self.last.insert(key.clone(), r.clone());
                    seen.insert(key);
                    output.push(r);
                }
                Ok(None) => {
                    if let Some(r) = self.cached(&key, c.clock.unix()) {
                        seen.insert(key);
                        output.push(r)
                    } else if ["playstation", "eightbitdo"].contains(&self.id) {
                        let mut r = Reading::new(&key, d.name, self.id, c.clock.unix());
                        r.kind = "gamepad".into();
                        r.via = if is_bluetooth(&i.path) {
                            "bluetooth"
                        } else {
                            "usb"
                        }
                        .into();
                        seen.insert(key);
                        output.push(r);
                    } else if self.id == "barracuda" {
                        let mut r = Reading::new(&key, d.name, self.id, c.clock.unix());
                        r.connection = Connection::Sleeping;
                        r.charging = Some(false);
                        r.kind = "headset".into();
                        seen.insert(key);
                        output.push(r);
                    } else if self.id == "audeze" {
                        self.first_seen.remove(&key);
                        let count = self.stuck.entry(key.clone()).or_default();
                        *count = if self.audeze_echo {
                            count.saturating_add(1)
                        } else {
                            0
                        };
                        if *count >= 2 {
                            let mut r = Reading::new(&key, d.name, self.id, c.clock.unix());
                            r.connection = Connection::Stale;
                            r.kind = "headset".into();
                            self.log("no answer from headset: unplug and replug dongle");
                            output.push(r);
                        }
                    }
                }
                Err(e) => {
                    self.log(e.to_string());
                    errors.push(e)
                }
            }
        }
        if self.id == "razer" {
            let live: Vec<_> = output
                .iter()
                .filter(|r| r.online())
                .map(|r| {
                    (
                        r.name.clone(),
                        r.key.split(':').nth(1).unwrap_or("").to_string(),
                    )
                })
                .collect();
            output.retain(|r| {
                r.online()
                    || !live.iter().any(|(name, pid)| {
                        name == &r.name && pid != r.key.split(':').nth(1).unwrap_or("")
                    })
            });
        }
        if self.id == "mchose" {
            for key in self.last.keys() {
                if !seen.contains(key)
                    && let Some(r) = self.cached(key, c.clock.unix())
                {
                    output.push(r)
                }
            }
        }
        if output.is_empty() && !errors.is_empty() {
            Err(errors.remove(0))
        } else {
            Ok(output)
        }
    }
}
