use hb_core::*;
use hb_providers::HidProvider;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
mod common;
use common::*;

#[test]
fn g7_uses_output_input_skips_noise_and_transient_error() {
    use hb_providers::protocols::padded;
    let hid = FakeHid::new(
        vec![info(0xa8a5, 0x2255, 0xff01)],
        vec![
            Step::Write(padded(&[0, 0x55, 0x30, 0xa5, 0x0b, 0x2e, 1, 1, 1], 65)),
            Step::Read(Ok(vec![8, 99])),
            Step::Read(Err("temporary disconnect")),
            Step::Read(Ok(vec![0xaa, 0x30, 0xa5, 0x0b, 10, 1, 1, 1, 46, 0])),
        ],
    );
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    let readings = HidProvider::new("mchose")
        .poll(&hid, &context(&clock, &cancel))
        .unwrap();
    assert_eq!(readings[0].level, Some(46));
    assert_eq!(readings[0].key, "mchose:a8a5");
    hid.done();
    assert_eq!(clock.monotonic(), Duration::from_millis(60));
}

#[test]
fn g7_does_not_write_to_other_chip_products_or_collections() {
    let hid = FakeHid::new(
        vec![info(0xa8a5, 0x9999, 0xff01), info(0xa8a5, 0x2255, 0xff05)],
        vec![],
    );
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    assert!(
        HidProvider::new("mchose")
            .poll(&hid, &context(&clock, &cancel))
            .unwrap()
            .is_empty()
    );
    assert!(hid.opened.lock().unwrap().is_empty());
}

fn mchose_steps(level: u8, charging: bool) -> Vec<Step> {
    let mut packet = vec![255; 21];
    packet[0] = 0x11;
    packet[1] = 0xf9;
    let mut response = vec![255; 21];
    response[0] = 0x11;
    response[1] = 0xf9;
    response[2] = 0xac;
    response[3] = 0xad;
    response[11] = level ^ 255;
    response[12] = u8::from(charging) ^ 255;
    vec![
        Step::Send(packet.clone()),
        Step::Feature(0x11, 21, Err("receiver pending")),
        Step::Send(packet),
        Step::Feature(0x11, 21, Ok(response)),
    ]
}
#[test]
fn mchose_vendor_family_includes_unknown_pids_and_prefers_charging_source() {
    let mut steps = mchose_steps(46, false);
    steps.extend(mchose_steps(47, true));
    let hid = FakeHid::new(
        vec![info(0x5253, 0x1020, 0xff01), info(0x5253, 0x31, 0xff01)],
        steps,
    );
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    let readings = HidProvider::new("mchose")
        .poll(&hid, &context(&clock, &cancel))
        .unwrap();
    assert_eq!(readings.len(), 1);
    assert_eq!(readings[0].level, Some(47));
    assert_eq!(readings[0].charging, Some(true));
    hid.done();
}

fn razer_reply(status: u8, cmd: u8, value: u8) -> Vec<u8> {
    let mut r = vec![0; 91];
    r[1] = status;
    r[7] = 7;
    r[8] = cmd;
    r[10] = value;
    r
}
#[test]
fn razer_retries_transaction_ids_and_caches_the_working_id() {
    use hb_providers::protocols::razer_request;
    let device = info(0x1532, 0x007c, 1);
    let preferred = hb_providers::catalog::DEVICES
        .iter()
        .find(|d| d.provider == "razer" && d.pid == device.product_id)
        .unwrap()
        .parameter;
    let alternate = [0x1f, 0x3f, 0xff, 0x9f, 8]
        .into_iter()
        .find(|tid| *tid != preferred)
        .unwrap();
    let mut steps = vec![
        Step::Send(razer_request(preferred, 0x80)),
        Step::Feature(0, 91, Ok(razer_reply(5, 0x80, 0))),
    ];
    for _ in 0..2 {
        steps.extend([
            Step::Send(razer_request(alternate, 0x80)),
            Step::Feature(0, 91, Ok(razer_reply(2, 0x80, 128))),
            Step::Send(razer_request(alternate, 0x84)),
            Step::Feature(0, 91, Ok(razer_reply(2, 0x84, 1))),
        ]);
    }
    let hid = FakeHid::new(vec![device], steps);
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    let mut provider = HidProvider::new("razer");
    for _ in 0..2 {
        let readings = provider.poll(&hid, &context(&clock, &cancel)).unwrap();
        assert_eq!(readings[0].level, Some(50));
        assert_eq!(readings[0].charging, Some(true));
    }
    hid.done();
}

#[test]
fn cancellation_prevents_opening_devices() {
    let hid = FakeHid::new(vec![info(0xa8a5, 0x2255, 0xff01)], vec![]);
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(true);
    assert!(
        HidProvider::new("mchose")
            .poll(&hid, &context(&clock, &cancel))
            .unwrap()
            .is_empty()
    );
    assert!(hid.opened.lock().unwrap().is_empty());
}

#[test]
fn sleeping_mchose_retains_reading_for_less_than_five_minutes() {
    let hid = FakeHid::new(vec![info(0x5253, 0x31, 0xff01)], mchose_steps(46, false));
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    let mut provider = HidProvider::new("mchose");
    let live = provider.poll(&hid, &context(&clock, &cancel)).unwrap();
    hid.done();
    let absent = FakeHid::new(vec![], vec![]);
    clock.0.store(299000, Ordering::Relaxed);
    let sleeping = provider.poll(&absent, &context(&clock, &cancel)).unwrap();
    assert_eq!(sleeping[0].connection, Connection::Sleeping);
    assert_eq!(sleeping[0].timestamp, live[0].timestamp);
    clock.0.store(300000, Ordering::Relaxed);
    assert!(
        provider
            .poll(&absent, &context(&clock, &cancel))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn expired_deadline_prevents_device_io() {
    let hid = FakeHid::new(vec![info(0xa8a5, 0x2255, 0xff01)], vec![]);
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    let context = PollContext {
        clock: &clock,
        cancelled: &cancel,
        deadline: Duration::ZERO,
        playstation_full_mode: false,
    };
    assert!(
        HidProvider::new("mchose")
            .poll(&hid, &context)
            .unwrap()
            .is_empty()
    );
    assert!(hid.opened.lock().unwrap().is_empty());
}

fn pa_reply(barracuda: bool, command: u8, value: u8) -> Vec<u8> {
    let mut r = vec![0; 64];
    r[0] = if barracuda { 1 } else { 2 };
    let c = if barracuda { 13 } else { 12 };
    r[c] = command;
    r[c + 1] = 1;
    r[c + 2] = 1;
    r[c + 3] = value;
    r
}

#[test]
fn blackshark_queries_wake_and_disable_remote_for_each_command() {
    use hb_providers::protocols::pa_request;
    let mut steps = vec![Step::Write(pa_request(false, 0xe1, Some(true)))];
    for (command, value) in [(0x21, 46), (0x2a, 1)] {
        steps.extend([
            Step::Read(Ok(vec![])),
            Step::Write(pa_request(false, 0xe1, Some(true))),
            Step::Write(pa_request(false, 0xe1, Some(true))),
            Step::Write(pa_request(false, command, None)),
            Step::Read(Ok(pa_reply(false, command, value))),
            Step::Write(pa_request(false, 0xe1, Some(false))),
        ]);
    }
    let hid = FakeHid::new(vec![info(0x1532, 0x0555, 0xff00)], steps);
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    let readings = HidProvider::new("razer")
        .poll(&hid, &context(&clock, &cancel))
        .unwrap();
    assert_eq!(readings[0].level, Some(46));
    assert_eq!(readings[0].charging, Some(true));
    assert_eq!(clock.monotonic(), Duration::from_millis(245));
    hid.done();
}

#[test]
fn barracuda_drains_and_disables_remote_after_matching_both_commands() {
    use hb_providers::protocols::pa_request;
    let mut steps = vec![
        Step::Read(Ok(vec![])),
        Step::Write(pa_request(true, 0xe1, Some(true))),
    ];
    for (command, value) in [(0x21, 46), (0x2a, 0)] {
        steps.extend([
            Step::Read(Ok(vec![])),
            Step::Write(pa_request(true, command, None)),
            Step::Read(Ok(pa_reply(true, command, value))),
        ]);
    }
    steps.extend([
        Step::Read(Ok(vec![])),
        Step::Write(pa_request(true, 0xe1, Some(false))),
    ]);
    let hid = FakeHid::new(vec![info(0x1532, 0x053a, 0xff00)], steps);
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    let readings = HidProvider::new("barracuda")
        .poll(&hid, &context(&clock, &cancel))
        .unwrap();
    assert_eq!(readings[0].level, Some(46));
    assert_eq!(readings[0].charging, Some(false));
    assert_eq!(clock.monotonic(), Duration::from_millis(50));
    hid.done();
}

fn hidpp_packet(feature: u8, token: u8, params: &[u8]) -> Vec<u8> {
    let mut r = hb_providers::protocols::padded(&[0x11, 255, feature, token], 20);
    r[4..4 + params.len()].copy_from_slice(params);
    r
}
fn hidpp_pair(feature: u8, token: u8, params: &[u8], reply: &[u8]) -> Vec<Step> {
    vec![
        Step::Write(hidpp_packet(feature, token, params)),
        Step::Read(Ok(hidpp_packet(feature, token, reply))),
    ]
}

#[test]
fn logitech_matches_tokens_reads_identity_and_reuses_complete_identity() {
    let mut steps = hidpp_pair(0, 0x1b, &[], &[2]);
    steps.extend(hidpp_pair(0, 0x0c, &[0, 5], &[2]));
    steps.extend(hidpp_pair(2, 0x0d, &[], &[5]));
    steps.extend(hidpp_pair(2, 0x1e, &[0], b"Mouse"));
    steps.extend(hidpp_pair(2, 0x2f, &[], &[3]));
    steps.extend(hidpp_pair(0, 0x0a, &[0, 3], &[3]));
    steps.extend(hidpp_pair(3, 0x0b, &[], &[0, 0xde, 0xad, 0xbe, 0xef]));
    steps.extend(hidpp_pair(0, 0x0c, &[0x10, 4], &[4]));
    steps.push(Step::Write(hidpp_packet(4, 0x1d, &[])));
    steps.push(Step::Read(Ok(hidpp_packet(4, 0x1c, &[99, 0, 0]))));
    steps.push(Step::Read(Ok(hidpp_packet(4, 0x1d, &[46, 0, 1]))));
    steps.extend(hidpp_pair(0, 0x1b, &[], &[2]));
    steps.extend(hidpp_pair(0, 0x0c, &[0x10, 4], &[4]));
    steps.extend(hidpp_pair(4, 0x1d, &[], &[47, 0, 0]));
    let mut device = info(0x046d, 0xc08b, 0xff00);
    device.usage = 2;
    let hid = FakeHid::new(vec![device], steps);
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    let mut provider = HidProvider::new("logitech");
    let first = provider.poll(&hid, &context(&clock, &cancel)).unwrap();
    assert_eq!(first[0].key, "logitech:DEADBEEF");
    assert_eq!(first[0].name, "Mouse");
    assert_eq!(first[0].level, Some(46));
    let second = provider.poll(&hid, &context(&clock, &cancel)).unwrap();
    assert_eq!(second[0].level, Some(47));
    hid.done();
}

#[test]
fn logitech_unknown_headset_collections_are_never_opened() {
    let hid = FakeHid::new(vec![info(0x046d, 0x9999, 0xff43)], vec![]);
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    assert!(
        HidProvider::new("logitech")
            .poll(&hid, &context(&clock, &cancel))
            .unwrap()
            .is_empty()
    );
    assert!(hid.opened.lock().unwrap().is_empty());
}

#[test]
fn audeze_uses_control_input_reports_and_newest_frame() {
    use hb_providers::protocols::padded;
    let mut device = info(0x3329, 0x4b18, 0xff13);
    device.serial = "headset-1".into();
    let hid = FakeHid::new(
        vec![device],
        vec![
            Step::Write(padded(&[6, 7, 0x80, 5, 0x5a, 3, 0, 0xd6, 0x0c], 62)),
            Step::Input(7, 62, Ok(vec![0xd6, 0x0c, 0, 0, 86])),
            Step::Input(7, 62, Ok(vec![0xd6, 0x0c, 0, 0, 85])),
            Step::Input(7, 62, Ok(vec![0xd6, 0x0c, 0, 0, 84])),
        ],
    );
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    let readings = HidProvider::new("audeze")
        .poll(&hid, &context(&clock, &cancel))
        .unwrap();
    assert_eq!(readings[0].level, Some(84));
    assert_eq!(readings[0].key, "audeze:headset-1");
    hid.done();
    assert_eq!(clock.monotonic(), Duration::from_millis(180));
}

#[test]
fn asus_error_frame_stops_retries_on_optional_zero_prefix() {
    use hb_providers::protocols::padded;
    let device = hb_providers::catalog::DEVICES
        .iter()
        .find(|d| d.provider == "asus")
        .unwrap();
    let hid = FakeHid::new(
        vec![info(device.vid, device.pid, 0xff00)],
        vec![
            Step::Read(Ok(vec![])),
            Step::Write(padded(&[0, 0x12, 7], 65)),
            Step::Read(Ok(vec![0, 0xff, 0xaa])),
        ],
    );
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    assert!(
        HidProvider::new("asus")
            .poll(&hid, &context(&clock, &cancel))
            .unwrap()
            .is_empty()
    );
    hid.done();
}

#[test]
fn pulsar_drains_then_accepts_only_checksum_valid_power_reports() {
    use hb_providers::protocols::pulsar_request;
    let device = hb_providers::catalog::DEVICES
        .iter()
        .find(|d| d.provider == "pulsar")
        .unwrap();
    let mut report = pulsar_request();
    report[6] = 46;
    report[7] = 1;
    report[16] = 0x55u8.wrapping_sub(report[..16].iter().fold(0u8, |a, b| a.wrapping_add(*b)));
    let mut selected = info(device.vid, device.pid, 0xff02);
    selected.usage = 2;
    selected.output_length = Some(17);
    let hid = FakeHid::new(
        vec![selected],
        vec![
            Step::Read(Ok(vec![])),
            Step::Write(pulsar_request()),
            Step::Read(Ok(vec![8, 4])),
            Step::Read(Ok(report)),
        ],
    );
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    let readings = HidProvider::new("pulsar")
        .poll(&hid, &context(&clock, &cancel))
        .unwrap();
    assert_eq!(readings[0].level, Some(46));
    hid.done();
    assert_eq!(clock.monotonic(), Duration::from_millis(20));
}

#[test]
fn lofree_closes_both_transactions_and_requires_online_ack() {
    use hb_providers::protocols::padded;
    let mut steps = Vec::new();
    for (command, value) in [(0xaa, 1), (0x1a, 46)] {
        steps.extend([
            Step::Write(padded(&[4, 0, 0, 1], 32)),
            Step::Read(Ok(vec![4, 0, 0, 1])),
            Step::Write(padded(&[4, 0, 0, command, 0, 0, 0, 0], 32)),
            Step::Read(Ok(vec![4, 0, 0, command, 0, 0, 0, 0, value])),
            Step::Write(padded(&[4, 0, 0, 2], 32)),
            Step::Read(Ok(vec![4, 0, 0, 2])),
        ]);
    }
    let mut selected = info(0x388d, 0x25, 0xff1c);
    selected.usage = 0x92;
    let hid = FakeHid::new(vec![selected], steps);
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    let readings = HidProvider::new("lofree")
        .poll(&hid, &context(&clock, &cancel))
        .unwrap();
    assert_eq!(readings[0].level, Some(46));
    hid.done();
}

#[test]
fn corsair_requires_heartbeat_and_drains_between_handshake_and_battery() {
    use hb_providers::protocols::padded;
    let device = hb_providers::catalog::DEVICES
        .iter()
        .find(|d| d.provider == "corsair" && d.variant != "nxp")
        .unwrap();
    let mut selected = info(device.vid, device.pid, 0);
    selected.interface = 4;
    let hid = FakeHid::new(
        vec![selected],
        vec![
            Step::Write(padded(&[0, 2, 8, 2, 0x13], 65)),
            Step::Write(padded(&[0, 2, 8, 2, 0x12], 65)),
            Step::Read(Ok(vec![])),
            Step::Write(padded(&[0, 2, 9, 2, 0x12], 65)),
            Step::Read(Ok(vec![1])),
            Step::Read(Ok(vec![])),
            Step::Write(padded(&[0, 2, 9, 2, 0x0f], 65)),
            Step::Read(Ok(vec![0, 0, 0, 0, 0xcc, 1])),
        ],
    );
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    let readings = HidProvider::new("corsair")
        .poll(&hid, &context(&clock, &cancel))
        .unwrap();
    assert_eq!(readings[0].level, Some(46));
    hid.done();
}

#[test]
fn nintendo_listens_then_sends_only_two_subcommands_with_wrapping_counter() {
    use hb_providers::protocols::padded;
    let mut device = info(0x057e, 0x2009, 1);
    device.path = "hid#vid&057e#bluetooth".into();
    let mut steps = vec![Step::Read(Ok(vec![])); 20];
    for counter in [0, 1] {
        steps.push(Step::Write(padded(
            &[1, counter, 0, 1, 0x40, 0x40, 0, 1, 0x40, 0x40, 2],
            49,
        )));
        steps.extend(vec![Step::Read(Ok(vec![])); 60]);
    }
    let hid = FakeHid::new(vec![device], steps);
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    assert!(
        HidProvider::new("nintendo")
            .poll(&hid, &context(&clock, &cancel))
            .unwrap()
            .is_empty()
    );
    hid.done();
    assert_eq!(clock.monotonic(), Duration::from_millis(700));
}

#[test]
fn playstation_basic_bluetooth_returns_immediately_and_usb_feature_is_read() {
    let mut bluetooth = info(0x054c, 0x0ce6, 1);
    bluetooth.path = "hid#vid&054c#bluetooth".into();
    let basic = FakeHid::new(vec![bluetooth], vec![Step::Read(Ok(vec![1, 0, 0]))]);
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    let readings = HidProvider::new("playstation")
        .poll(&basic, &context(&clock, &cancel))
        .unwrap();
    assert_eq!(readings[0].level, None);
    assert_eq!(clock.monotonic(), Duration::ZERO);
    basic.done();
    let mut report = vec![0; 64];
    report[0] = 1;
    report[53] = 0x17;
    let usb = FakeHid::new(
        vec![info(0x054c, 0x0ce6, 1)],
        vec![Step::Feature(5, 64, Ok(vec![])), Step::Read(Ok(report))],
    );
    let readings = HidProvider::new("playstation")
        .poll(&usb, &context(&clock, &cancel))
        .unwrap();
    assert_eq!(readings[0].level, Some(70));
    assert_eq!(readings[0].charging, Some(true));
    usb.done();
}

#[test]
fn eightbitdo_is_passive_and_uses_distinct_usb_report_id() {
    let device = hb_providers::catalog::DEVICES
        .iter()
        .find(|d| d.provider == "eightbitdo")
        .unwrap();
    let mut report = vec![0; 64];
    report[0] = 4;
    report[14] = 46 | 128;
    let hid = FakeHid::new(
        vec![info(device.vid, device.pid, 1)],
        vec![Step::Read(Ok(vec![1, 0, 0])), Step::Read(Ok(report))],
    );
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    let readings = HidProvider::new("eightbitdo")
        .poll(&hid, &context(&clock, &cancel))
        .unwrap();
    assert_eq!(readings[0].level, Some(46));
    assert_eq!(readings[0].charging, Some(true));
    hid.done();
}

#[test]
fn razer_foreign_responses_cost_three_sends_and_cached_collection_is_reused() {
    use hb_providers::protocols::razer_request;
    let device = info(0x1532, 0x007c, 1);
    let tid = hb_providers::catalog::DEVICES
        .iter()
        .find(|d| d.provider == "razer" && d.pid == device.product_id)
        .unwrap()
        .parameter;
    let mut steps = Vec::new();
    for _ in 0..2 {
        steps.push(Step::Send(razer_request(tid, 0x80)));
        for index in 1..=13 {
            let mut foreign = razer_reply(2, 3, 99);
            foreign[7] = 0x0f;
            steps.push(Step::Feature(0, 91, Ok(foreign)));
            if [4, 8].contains(&index) {
                steps.push(Step::Send(razer_request(tid, 0x80)));
            }
        }
    }
    let hid = FakeHid::new(vec![device], steps);
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    let mut provider = HidProvider::new("razer");
    for _ in 0..2 {
        assert!(
            provider
                .poll(&hid, &context(&clock, &cancel))
                .unwrap()
                .is_empty()
        );
    }
    hid.done();
    assert_eq!(clock.monotonic(), Duration::from_millis(2040));
}

#[test]
fn remaining_hid_families_use_the_reference_channel_packet_and_positive_reply() {
    use hb_providers::protocols::padded;
    for family in [
        "wlmouse",
        "lamzu",
        "gwolves",
        "astro",
        "hyperx",
        "hyperx_cloud3",
        "hyperx_alpha2",
        "keychron",
        "jbl",
        "am_infinity",
    ] {
        let known = hb_providers::catalog::DEVICES
            .iter()
            .find(|d| d.provider == family)
            .unwrap();
        let mut selected = info(known.vid, known.pid, 0);
        let steps = match family {
            "wlmouse" | "lamzu" | "gwolves" => {
                selected.usage_page = 0xffff;
                selected.interface = 2;
                selected.feature_length = Some(65);
                vec![
                    Step::Send(padded(&[0, 0, 0, 2, 2, 0, 0x83], 65)),
                    Step::Feature(0, 65, Ok(vec![0xa1, 0, 0, 2, 0, 0x83, 0, 46])),
                ]
            }
            "astro" => {
                selected.usage_page = 0xff32;
                selected.usage = 0x74;
                vec![
                    Step::Write(padded(&[2, 12, 3, 0, 6, 12], 64)),
                    Step::Read(Ok(vec![2, 12, 0, 0, 6, 0, 46, 0, 0])),
                ]
            }
            "hyperx" => {
                selected.usage_page = 0xff90;
                selected.usage = 0x303;
                vec![
                    Step::Write(padded(&[6, 255, 187, 2, 0], 52)),
                    Step::Read(Ok(vec![6, 255, 187, 2, 0, 0, 0, 46])),
                    Step::Write(padded(&[6, 255, 187, 3, 0], 52)),
                    Step::Read(Ok(vec![6, 255, 187, 3, 0])),
                ]
            }
            "hyperx_cloud3" => {
                selected.usage_page = 0xff13;
                vec![
                    Step::Write(padded(&[0x66, 0x89], 62)),
                    Step::Read(Ok(vec![0x66, 0x0d, 1, 0, 46])),
                    Step::Write(padded(&[0x66, 0x8a], 62)),
                    Step::Read(Ok(vec![0x66, 0x0c, 0])),
                ]
            }
            "hyperx_alpha2" => {
                selected.usage_page = 0xff13;
                selected.usage = 0xff00;
                vec![
                    Step::Read(Ok(vec![])),
                    Step::Write(padded(&[0x50, 2], 64)),
                    Step::Read(Ok(vec![0x51, 2, 46, 0, 0, 0, 0])),
                ]
            }
            "keychron" => {
                selected.interface = 4;
                let mut reply = vec![0; 64];
                reply[0] = 0xb4;
                reply[1] = 6;
                reply[20] = 46;
                vec![Step::Send(padded(&[0xb3, 6], 64)), Step::Read(Ok(reply))]
            }
            "jbl" => {
                selected.usage_page = 0xff13;
                vec![Step::Read(Ok(vec![0x2f, 99])), Step::Read(Ok(vec![8, 46]))]
            }
            "am_infinity" => {
                selected.usage_page = 0xffff;
                selected.usage = 2;
                vec![
                    Step::Send(padded(&[0, 0xf7], 65)),
                    Step::Feature(5, 65, Ok(vec![5, 0, 0, 0])),
                    Step::Send(padded(&[0, 0xf7], 67)),
                    Step::Feature(5, 65, Ok(vec![5, 0, 0, 46])),
                ]
            }
            _ => unreachable!(),
        };
        let hid = FakeHid::new(vec![selected], steps);
        let clock = FakeClock::default();
        let cancel = AtomicBool::new(false);
        let readings = HidProvider::new(family)
            .poll(&hid, &context(&clock, &cancel))
            .unwrap();
        assert_eq!(readings.len(), 1, "{family}");
        assert_eq!(readings[0].level, Some(46), "{family}");
        assert_eq!(readings[0].charging, Some(false), "{family}");
        hid.done();
    }
}

#[test]
fn candidate_fallback_and_positive_capacity_selection_are_reference_policies() {
    use hb_providers::protocols::{padded, pulsar_request};
    let known = hb_providers::catalog::DEVICES
        .iter()
        .find(|d| d.provider == "astro")
        .unwrap();
    let hid = FakeHid::new(
        vec![info(known.vid, known.pid, 1)],
        vec![
            Step::Write(padded(&[2, 12, 3, 0, 6, 12], 64)),
            Step::Read(Ok(vec![2, 12, 0, 0, 6, 0, 46, 0, 0])),
        ],
    );
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    assert_eq!(
        HidProvider::new("astro")
            .poll(&hid, &context(&clock, &cancel))
            .unwrap()[0]
            .level,
        Some(46)
    );
    hid.done();
    let known = hb_providers::catalog::DEVICES
        .iter()
        .find(|d| d.provider == "pulsar")
        .unwrap();
    let mut bad = info(known.vid, known.pid, 0xff02);
    bad.path = "wrong-size-control".into();
    bad.usage = 2;
    bad.output_length = Some(8);
    let mut good = info(known.vid, known.pid, 1);
    good.path = "fits".into();
    good.output_length = Some(17);
    let mut reply = pulsar_request();
    reply[6] = 46;
    reply[16] = 0x55u8.wrapping_sub(reply[..16].iter().fold(0u8, |a, b| a.wrapping_add(*b)));
    let hid = FakeHid::new(
        vec![bad, good],
        vec![
            Step::Read(Ok(vec![])),
            Step::Write(pulsar_request()),
            Step::Read(Ok(reply)),
        ],
    );
    assert_eq!(
        HidProvider::new("pulsar")
            .poll(&hid, &context(&clock, &cancel))
            .unwrap()[0]
            .level,
        Some(46)
    );
    assert_eq!(*hid.opened.lock().unwrap(), vec!["fits".to_string()]);
    hid.done();
}

#[test]
fn unknown_product_ids_and_wrong_strict_collections_never_get_commands() {
    for &(family, _) in hb_providers::provider::FAMILIES {
        let known = hb_providers::catalog::DEVICES
            .iter()
            .find(|d| d.provider == family)
            .unwrap();
        let vid = if family == "mchose" {
            0xa8a5
        } else {
            known.vid
        };
        let mut unknown = info(vid, 0x9999, 0xff43);
        unknown.product = "wireless receiver".into();
        let hid = FakeHid::new(vec![unknown], vec![]);
        let clock = FakeClock::default();
        let cancel = AtomicBool::new(false);
        assert!(
            HidProvider::new(family)
                .poll(&hid, &context(&clock, &cancel))
                .unwrap()
                .is_empty(),
            "{family}"
        );
        assert!(hid.opened.lock().unwrap().is_empty(), "{family}");
    }
    for family in [
        "hyperx",
        "hyperx_cloud3",
        "hyperx_alpha2",
        "asus",
        "lamzu",
        "gwolves",
        "lofree",
        "am_infinity",
        "barracuda",
        "steelseries",
    ] {
        let known = hb_providers::catalog::DEVICES
            .iter()
            .find(|d| d.provider == family)
            .unwrap();
        let mut wrong = info(known.vid, known.pid, 1);
        wrong.interface = 9;
        let hid = FakeHid::new(vec![wrong], vec![]);
        let clock = FakeClock::default();
        let cancel = AtomicBool::new(false);
        assert!(
            HidProvider::new(family)
                .poll(&hid, &context(&clock, &cancel))
                .unwrap()
                .is_empty(),
            "{family}"
        );
        assert!(hid.opened.lock().unwrap().is_empty(), "{family}");
    }
}

#[test]
fn lofree_start_reply_and_stop_share_a_single_three_second_deadline() {
    use hb_providers::protocols::padded;
    let mut steps = vec![Step::Write(padded(&[4, 0, 0, 1], 32))];
    steps.extend(vec![Step::Read(Ok(vec![])); 29]);
    steps.extend([
        Step::Read(Ok(vec![4, 0, 0, 1])),
        Step::Write(padded(&[4, 0, 0, 0xaa, 0, 0, 0, 0], 32)),
        Step::Read(Ok(vec![])),
        Step::Write(padded(&[4, 0, 0, 2], 32)),
    ]);
    let mut selected = info(0x388d, 0x25, 0xff1c);
    selected.usage = 0x92;
    let hid = FakeHid::new(vec![selected], steps);
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    assert!(
        HidProvider::new("lofree")
            .poll(&hid, &context(&clock, &cancel))
            .unwrap()
            .is_empty()
    );
    hid.done();
    assert_eq!(clock.monotonic(), Duration::from_secs(3));
}
#[test]
fn unopenable_eightbitdo_still_reports_controller_without_battery() {
    let known = hb_providers::catalog::DEVICES
        .iter()
        .find(|d| d.provider == "eightbitdo")
        .unwrap();
    let selected = info(known.vid, known.pid, 1);
    let hid =
        FakeHid::new(vec![selected.clone()], vec![]).fail_open(&selected.path, "access denied");
    let clock = FakeClock::default();
    let cancel = AtomicBool::new(false);
    let readings = HidProvider::new("eightbitdo")
        .poll(&hid, &context(&clock, &cancel))
        .unwrap();
    assert_eq!(readings.len(), 1);
    assert_eq!(readings[0].level, None);
    assert_eq!(readings[0].kind, "gamepad");
    assert!(readings[0].online());
}
