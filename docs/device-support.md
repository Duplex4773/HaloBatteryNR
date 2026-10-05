# Device verification status

Inherited battery-monitoring evidence from the parent application's checked-in
support table (reviewed 2 October 2026). **User-verified in parent app** means
the parent reports a real battery reading on the listed device/connection,
including maintainer or user reports. It is not a new test of the Rust port.
The independent rewrite inherits device protocols, provider behavior and test
evidence. Matching IDs alone do not establish support. The inherited reports
retain their scope without upgrading other
models, transports or features in the same family.

The wireless DeathAdder V4 Pro additionally has Rust-port hardware checks and
user-verified polling configuration at 125, 500, 1000, 2000, 4000 and 8000 Hz,
confirmed 2 October 2026. This confirms configured-rate changes on that wireless
setup; wired operation and independently measured effective USB frequency remain
unverified. It is not manufacturer or anti-cheat certification.
Parent battery verification does **not** verify polling-rate writes. See
[polling support](polling-controls.md) for that separate feature's evidence.

| Device | Connection | Battery verification |
| --- | --- | --- |
| [8BitDo Pro 2, Pro 3, SN30 Pro, SF30 Pro in D-input mode](protocols.md#8bitdo-pro-2-pro-3-sn30-pro-sf30-pro-in-d-input-mode) | Bluetooth or USB | Hardware unverified |
| [AM Infinity 8K (Angry Miao)](protocols.md#am-infinity-8k-angry-miao) | 2.4 GHz receiver | Hardware unverified |
| [Astro A50 Gen 5 (Logitech 046D:0B1C)](protocols.md#astro-a50-gen-5-logitech-046d0b1c) | Base station | Hardware unverified |
| [ASUS ROG Gladius III Aimpoint and other ROG / TUF wireless mice (parent model list)](protocols.md#asus-rog-gladius-iii-aimpoint-and-other-rog--tuf-wireless-mice) | 2.4 GHz receiver or USB cable | Hardware unverified |
| [Audeze Maxwell](protocols.md#audeze-maxwell) | 2.4 GHz dongle or USB-C cable | User-verified in parent app |
| [Bluetooth devices, tested on the 1MORE SonoFlow headset (users also report Audio-Technica and JBL Tune 760NC headphones working)](protocols.md#bluetooth-devices-tested-on-the-1more-sonoflow-headset) | Bluetooth (on by default, can be turned off in the menu) | User-verified in parent app |
| [Corsair Dark Core RGB Pro SE](protocols.md#corsair-dark-core-rgb-pro-se) | 2.4 GHz dongle | Hardware unverified |
| [Corsair Void v2 Wireless, Virtuoso Max Wireless, HS80 Max Wireless](protocols.md#corsair-void-v2-wireless-virtuoso-max-wireless-hs80-max-wireless) | Wireless receiver | Hardware unverified |
| [GameSir G7 Pro; FlyDigi Vader Pro (tested by users)](protocols.md#gamesir-g7-pro-flydigi-vader-pro) | 2.4 GHz receiver (shows up as an Xbox controller) | User-verified in parent app |
| [G-Wolves WARG, HTS Plus (Pro), HTXU, Lycan, Fenrir Pro / Asym, HTX Mini](protocols.md#g-wolves-warg-hts-plus-pro-htxu-lycan-fenrir-pro--asym-htx-mini) | 8K receiver or USB cable | Hardware unverified |
| [Hitscan Hyperlight](protocols.md#hitscan-hyperlight) | 2.4 GHz receiver or USB cable | Hardware unverified |
| [HyperX Cloud Alpha 2](protocols.md#hyperx-cloud-alpha-2) | 2.4 GHz station | User-verified in parent app |
| [HyperX Cloud II Wireless](protocols.md#hyperx-cloud-ii-wireless) | 2.4 GHz dongle | Hardware unverified |
| [HyperX Cloud III Wireless](protocols.md#hyperx-cloud-iii-wireless) | 2.4 GHz dongle | Hardware unverified |
| [JBL Quantum 910 Wireless](protocols.md#jbl-quantum-910-wireless) | 2.4 GHz dongle | User-verified in parent app |
| [Keychron Ultra-Link 8K, Keychron M5](protocols.md#keychron-ultra-link-8k-keychron-m5) | 2.4 GHz receiver and USB cable | Hardware unverified |
| [LAMZU Maya X](protocols.md#lamzu-maya-x) | 8K dongle or USB cable | User-verified in parent app |
| [Lofree Hyzen](protocols.md#lofree-hyzen) | 2.4 GHz dongle | Hardware unverified |
| [Logitech G502 LIGHTSPEED, G502 X PLUS](protocols.md#logitech-g502-lightspeed-g502-x-plus) | Lightspeed receiver | User-verified in parent app |
| [Logitech (more HID++ 2.0 devices and G-series headsets)](protocols.md#logitech-more-hid-20-devices-and-g-series-headsets) | Lightspeed, Unifying or Bolt receiver | Expected support; hardware unverified |
| [MCHOSE A7 V2 Ultra](protocols.md#mchose-a7-v2-ultra) | 2.4 GHz receiver | Hardware unverified |
| [MCHOSE G7](protocols.md#mchose-g7) | USB (chip 'YJX-CHIP') | User-verified in parent app |
| [MCHOSE M7 Ultra](protocols.md#mchose-m7-ultra) | 2.4 GHz receiver | User-verified in parent app |
| [Nintendo Switch Pro Controller, Joy-Con (L) / (R)](protocols.md#nintendo-switch-pro-controller-joy-con-l--r) | Bluetooth | Hardware unverified |
| [Pulsar X2 V2 Mini, ATK VXE R1 SE+, VXE R1 Pro Max](protocols.md#pulsar-x2-v2-mini-atk-vxe-r1-se-vxe-r1-pro-max) | 2.4 GHz dongle and USB cable | User-verified in parent app |
| [Razer Barracuda Pro (2.4 GHz)](protocols.md#razer-barracuda-pro-24-ghz) | 2.4 GHz dongle | User-verified in parent app |
| [Razer Basilisk V3 Pro, Razer Basilisk Ultimate (tested by users)](protocols.md#razer-basilisk-v3-pro-razer-basilisk-ultimate) | 2.4 GHz receiver | User-verified in parent app |
| [Razer BlackShark V2 Pro (2023)](protocols.md#razer-blackshark-v2-pro-2023) | 2.4 GHz receiver | User-verified in parent app |
| [Razer BlackWidow V3 Pro](protocols.md#razer-blackwidow-v3-pro) | 2.4 GHz receiver or USB cable | Hardware unverified |
| [Razer DeathAdder V4 Pro](protocols.md#razer-deathadder-v4-pro) | 2.4 GHz receiver | User-verified in parent app |
| [Other Razer wireless mice](protocols.md#razer-wireless-mice-other-openrazer-models) | 2.4 GHz receiver or USB cable | Expected support; hardware unverified |
| [Sony DualSense (PS5)](protocols.md#sony-dualsense-ps5) | USB or Bluetooth | User-verified in parent app |
| [Sony DualShock 4 (PS4)](protocols.md#sony-dualshock-4-ps4) | USB cable and Bluetooth | User-verified in parent app |
| [SteelSeries Aerox 3 Wireless](protocols.md#steelseries-aerox-3-wireless) | 2.4 GHz dongle | Hardware unverified |
| [SteelSeries Arctis and GameBuds (other models)](protocols.md#steelseries-arctis-and-gamebuds-other-models) | wireless base station or dongle | Expected support; hardware unverified |
| [SteelSeries Arctis Nova 7](protocols.md#steelseries-arctis-nova-7) | 2.4 GHz dongle | User-verified in parent app |
| [SteelSeries Arctis Nova Pro Wireless (`1038:12E0`, `1038:12E5` X)](protocols.md#steelseries-arctis-nova-pro-wireless-103812e0-103812e5-x) | Wireless base station, interface 3 or 4 | Hardware unverified |
| [SteelSeries Rival 3 Wireless](protocols.md#steelseries-rival-3-wireless) | 2.4 GHz dongle | Hardware unverified |
| [WLmouse Beast X and Beast X Mini Pro](protocols.md#wlmouse-beast-x-and-beast-x-mini-pro) | 8K or 1K receiver, or USB cable | Expected support; hardware unverified |
| [WLmouse Beast X Max](protocols.md#wlmouse-beast-x-max) | 8K receiver and USB cable | User-verified in parent app |
| [Xbox-compatible controllers (other models)](protocols.md#xbox-compatible-controllers-other-models) | USB or the Xbox wireless adapter | Expected support; hardware unverified |

Keep the parent table's exact model and connection scope when updating these
labels. Do not promote `likely` or `no` to verified based solely on matching IDs
or simulated tests. Existing PlayStation full-report and 8BitDo mode limitations
continue to apply; see [protocol details](protocols.md).

Thanks to HeyOkay and the original contributors and hardware testers for these
reports and the battery protocols that make the port possible.

## Added from upstream 1.14.0

These rows inherit the exact parent-app report scope. All Rust tests for these
additions are simulated; no new local hardware verification is claimed.

| Device | Connection | Evidence |
| --- | --- | --- |
| [HyperX Cloud III S Wireless](protocols.md#hyperx-cloud-iii-s-wireless) (`03F0:02CC`, `03F0:06BE`) | 2.4 GHz dongle | User-verified in parent app, both dongles |
| [Logitech G PRO X 2 LIGHTSPEED](protocols.md#logitech-g-pro-x-2-lightspeed) (`046D:0AF7`) | 2.4 GHz receiver | User-verified in parent app |
| [SteelSeries Arctis Nova Elite](protocols.md#steelseries-arctis-nova-elite) (`1038:2244`) | Wireless base station | User-verified in parent app |
| [Razer DeathStalker V2 Pro TKL](protocols.md#razer-wireless-keyboards-added-in-1140) | HyperSpeed receiver or cable | User-verified in parent app |
| [Razer DeathStalker V2 Pro, BlackWidow V3 Mini, V4 Mini and V4 Tenkeyless HyperSpeed](protocols.md#razer-wireless-keyboards-added-in-1140) | HyperSpeed receiver or cable | Hardware unverified |
| [G-Wolves models with their own receiver](protocols.md#g-wolves-model-specific-receivers) | Matching receiver or cable | Hardware unverified |

Battery monitoring additions do not expand polling-rate write permissions.
