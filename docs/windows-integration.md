# Windows integration

`hb-windows` uses native Win32/WinRT APIs and HIDAPI's Windows C backend; it does not run PowerShell. Polling is read-only except for HID provider requests that are controlled by the protocol allowlists in `hb-providers`.

## Resource lifetime and HID discovery

HID enumeration is cached per vendor for 30 seconds and invalidated by device events or an open/enumeration failure. HIDAPI enumerates with zero desired access. The extra native capabilities probe also opens with zero desired access and shared read/write; inaccessible or exclusive collections can retain their identity without invented report lengths. Native handles and HID preparsed data are owned by Rust drop guards. Configuration Manager container IDs support physical-device grouping. HID sessions are HIDAPI-owned and close on drop.

## Bluetooth

PnP root and service nodes are grouped by MAC, including service-node IDs whose address is not prefixed by `DEV_`. The root friendly name takes priority. Battery properties accept byte or DWORD values only in the 0–100 range. Last known battery values survive temporary property omission and are marked stale. Classic Class of Device, LE Appearance, and audio service UUIDs determine device kind.

Classic/LE WinRT objects are cached per MAC. Each object owns a ConnectionStatusChanged event token, unregisters its handler, and calls Close when removed or dropped. New async object queries have a three-second bound and honor the poll deadline/cancellation. Failed connection queries use the original PnP connection flag when available and otherwise hide the device; a missing connection answer is never treated as connected merely because its PnP node exists. Diagnostics expose these failures.

The provider checks its event flag at most every two seconds. Arrival/manual invalidation and connection events schedule full snapshots at 0, 3, 8, and 15 seconds, then once per minute. Intermediate polls return the cached readings without device enumeration. An unusable/cancelled PnP snapshot returns an error, while a successful empty snapshot returns an empty list; the shared engine handles failure retention and missing-device smoothing.

## Controllers

WGI gamepads retain their NonRoamableId identity. Real capacity percentages are trusted for the original Microsoft/GameSir vendor allowlist. Known Microsoft Bluetooth product IDs deliberately do not use WGI capacity: their reported percentages can be incorrect; the Bluetooth provider supplies the battery. Other WGI gamepads retain connection and charging status with an unknown level. XInput always checks all four connected slots and retains each slot independently. Its coarse levels match the original provider: 5/20/55/100. Wired, unknown, disconnected battery types and invalid coarse bytes never fabricate a percentage or charging state.

Windows provides no dependable WGI-to-XInput slot identity in these APIs. The port retains both identities rather than pairing by name, list position, or controller count. The same physical controller can therefore appear twice when both APIs report it. This preserves unrelated controllers but is a known limitation; safe deduplication requires a proven physical identity mapping. A wired XInput type can also describe a wireless receiver, so the port does not infer charging from that flag.

## Shell integration and validation

Startup registration, application identity, and the single-instance mutex use the distinct HaloBatteryNext identity. Windows theme and fullscreen gaming checks are native. MyDockFinder process detection uses an owned Toolhelp snapshot, a ten-second cache while present and a thirty-second cache while absent. Explicit theme/settings refreshes and Explorer recovery bypass it. Borrowed UTF-16 comparisons recognize the original executable names and names containing `mydock` without per-process string allocations.

The mutex reports already-running separately from actual acquisition failures.
A normal second launch posts an Open request to the existing monitor; a duplicate
background launch exits quietly. Dashboard handles are checked for validity and
ownership before reuse. The titlebar close path releases the mutable state borrow
before calling the default window procedure, allowing its nested close message to
run normal cleanup. Explicit reentrant close/destruction paths defer targeted
cleanup to the monitor. Closing releases controls, chart, brushes and font without
exiting monitoring; stale destruction notifications cannot retire a recreated
dashboard. See the documented [default window procedure](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-defwindowprocw)
and [window-handle validity](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-iswindow).

Dashboard appearance reads Windows `AppsUseLightTheme`, separately from the
taskbar's `SystemUsesLightTheme`. Settings/theme/system-color messages update
owned GDI brushes, native controls and Direct2D colors without rebuilding unsaved
edits. High contrast uses Windows system colors and native contrast behavior.
Failed preference reads default to light. Background painting explicitly fills
the dirty rectangle after `BeginPaint`, whose nested erase request can occur
while UI state is borrowed. Client printing uses the same owned palette brush.
The titlebar uses documented [DWM attributes](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute);
native control painting uses documented subclass, owner-draw and control-color
messages, retaining Windows keyboard, focus and accessibility behavior. No
undocumented theme ordinals, global input hooks or system preference writes are
used. Dashboard-only resources are released when it closes.

The optional orange warning band defaults to 30% and is independent of low-charge
notifications. Charging green and per-device low-alert red retain priority;
zero disables orange. Theme/threshold changes update registered icons in place.

Device tray GUIDs remain stable through sleep and wake. Battery, theme, DPI and
settings changes modify the existing notification icon instead of deleting and
adding it again. Explorer recovery first attempts a modification and adds the
same GUID only after that fails; hiding/removing a device and shutdown still
unregister its icon. This follows the separate add/modify/delete operations in
[Microsoft's Shell_NotifyIcon documentation](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyiconw).
Windows retains control of main-area versus overflow placement; the app does
not override user preferences or modify Explorer's registry settings.

Known Razer mouse identity remains present while its exact allowlisted HID
collection is enumerated, including beyond the five-minute cached-percentage
timeout. At expiry its percentage and charging state become unknown, preserving
original freshness and its registered tray icon. An unplugged receiver is still
removed through normal engine miss handling. Explicit communication failures
remain provider errors, while error-free backoff preserves known presence.

`cargo check -p hb-windows` and `cargo test -p hb-windows` pass. Tests cover MAC/service grouping boundaries, Bluetooth classification, coarse/unknown/wired battery interpretation, invalid battery DWORD values, arrival retry deadlines, and cached polling with a fake Clock and HidTransport. No connected hardware or HID writes are required by these tests. Actual battery properties, device connection events, exclusive-device behavior, and multi-controller identity still require Windows hardware smoke tests. Full MyDockFinder wallpaper/window luminance matching is handled separately by the UI and is not established by process presence alone.
