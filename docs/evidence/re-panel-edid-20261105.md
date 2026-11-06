# Panel identity from EDID — channel, evidence, implementation (TB14P-EZRA, 2026-11-05)

Target: Lenovo 21VG (ThinkBook 14 G8+ IPH), internal panel. Session: **read-only**
SSH network logon. Nothing was written: no registry value, no display mode, no
vendor binary was executed, and no WMI method was invoked. Everything below comes
from reading two OS-owned stores (`root\wmi` class instances and the PnP
enumeration registry) and decoding the bytes in them.

This note documents the contract `panel info` implements. The Rust in this
repository was **not** compiled or run by this work; a source edit is not
verified behavior — §8 lists what the parent must exercise on the target.

## 1. Channel map

| step | source | observation |
| --- | --- | --- |
| select the embedded panel | `root\wmi` `WmiMonitorConnectionParams` | exactly one instance: `DISPLAY\LEN8AD7\4&3ab355bd&1&UID8388688_0`, `VideoOutputTechnology` = `2147483648` = `0x80000000` = `D3DKMDT_VOT_INTERNAL` |
| read its EDID | `HKLM\SYSTEM\CurrentControlSet\Enum\DISPLAY\LEN8AD7\4&3ab355bd&1&UID8388688\Device Parameters` value `EDID` | `REG_BINARY`, 384 bytes, base block + 2 extension blocks |
| strip the WMI display index | `InstanceName` suffix `_0` | device instance `DISPLAY\LEN8AD7\4&3ab355bd&1&UID8388688` = the registry key, byte for byte |
| cross-check the instance | monitor PnP class name | class `LEN8AD7` = manufacturer `LEN` + product `0x8AD7` decoded from the same block |

Reproduction (read-only, PowerShell over SSH; `Device Parameters` carries no
other value on this instance, only the `WDF` subkey):

```powershell
Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorConnectionParams |
  ForEach-Object { "$($_.InstanceName) tech=$($_.VideoOutputTechnology)" }
(Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Enum\DISPLAY\LEN8AD7\4&3ab355bd&1&UID8388688\Device Parameters' -Name EDID).EDID
```

Channels that do **not** work on this host, and why the selection is not built on
them:

- `WmiMonitorID`, `WmiMonitorBasicDisplayParams`, `WmiMonitorListedSupportedSourceModes`
  enumerate **zero** instances through this transport (`Get-CimInstance ... -ErrorAction Stop`
  completed, `count=0`), so no vendor-name/serial/active-flag cross-check is
  available from those classes. Only `WmiMonitorConnectionParams` answered.
- Display-path APIs describe the *calling session's* desktop. Through this SSH
  logon, `[System.Windows.Forms.Screen]::AllScreens` reports a single virtual
  `WinDisc` screen at `1024x768` (primary), not the internal panel, so a
  display-path enumeration is not a usable selector for this transport. (The
  observation is the session's screen enumeration; a `QueryDisplayConfig` call
  was not made.)
- `ConfigFlags` is `0` on every `DISPLAY` instance (including the stale ones), so
  "present" cannot be inferred from the enum record either.

## 2. Captured value

`EDID` of `DISPLAY\LEN8AD7\4&3ab355bd&1&UID8388688` (384 bytes, block sums
`[0, 0, 0]`, extension count byte 126 = `0x02`):

SHA-256 of the 384-byte value:
`648806e959b417cc81049eb1d1e6e2c33c084b09514ce8a211ea0c06de1c0bcc`.
The byte-exact value is pinned by the unit test constant
`crates/lctrl-core/src/edid.rs::tests::REFERENCE_EDID` (verified equal to this
capture), and the base64 below decodes to it:

```text
AP///////wAwrteKAAAAAAAjAQS1HxR4Aw+VrlJDsCYPUFQAAAABAQEBAQEBAQEBAQEBAQEBGJkAoMCAZXAwIGYAOMMQAAAYAAAA/QAeePDwTgEKICAgICAgAAAA/gBMRU5PVk8KICAgICAgAAAA/ABMRU4xNC41M0sKICAgAskCAw8A4wWAgOYGBQFqakgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA9nAgeQIAIAAMQIv2bRQAAAAAABkAIQAdMQyfBwAMgAeA4fpRPaSwZmIPAkVU0F/QXwBFE3giABTW9QsF/wufAC8AHwB/B2QABQAFACYACQYAAAAAAFAAAC4ABgBF0F/QX4EAFXQaAAADUx54ACBr/2v/eAAAAACtAAAAAAAAAMyQ
```

Block tags: base block; extension 1 `0x02` (CTA-861, revision 3, 15-byte DTD
offset); extension 2 `0x70` (DisplayID).

## 3. Decoded identity

| field | bytes | derivation | value |
| --- | --- | --- | --- |
| manufacturer | 8–9 | `0x30AE` → five-bit letters `01100 00101 01110` → `12,5,14` | `LEN` |
| product code | 10–11 | little-endian `D7 8A` | `0x8AD7` |
| serial number | 12–15 | `00 00 00 00` = unused, not "serial 0" | absent |
| serial text | `0xFF` descriptor | none present | absent |
| manufacture week | 16 | `0x00` = week not specified | absent |
| manufacture year | 17 | `0x23` = 35, +1990 | 2025 |
| EDID version | 18–19 | `01 04` | 1.4 |
| video input | 20 | `0xB5` = `1 011 0101`: digital, bit depth code `3` = 10 bits per primary, interface code `5` = DisplayPort | digital 10 bpc, DP |
| panel size | 21–22 | `0x1F`, `0x14` (cm) | 310 × 200 mm |
| range limits | `0xFD` descriptor | `1E 78 F0 F0 4E` | 30–120 Hz vertical, 240–240 kHz horizontal, ≤ 780 MHz pixel clock |
| product name | `0xFE` descriptor | `4C454E4F564F` | `LENOVO` |
| monitor name | `0xFC` descriptor | `4C454E31342E35334B` | `LEN14.53K` |
| preferred timing | DTD 1 | clock `0x9918` × 10 kHz = 391.92 MHz; H `3072 + 160 = 3232`, V `1920 + 101 = 2021` → 60.00 Hz | 3072 × 1920 @ 60 Hz |
| connection | input byte + OS class | interface `5` (DisplayPort) on the instance the OS classifies `VOT_INTERNAL` | eDP |

Physical diagonal: `sqrt(310² + 200²) = 368.9 mm ≈ 14.5 in`, consistent with the
`LEN14.53K` name descriptor. The preferred timing is the highest-resolution
3072 × 1920 mode at 60 Hz; the range limits allow up to 120 Hz (the vendor's
120 Hz mode is carried by the firmware panel-refresh block, not by this DTD).

Two honest notes about the raw data:

- The `0xFD` descriptor stores **equal** minimum and maximum horizontal rate
  (240 kHz). That is what the panel advertises; the implementation reports the
  bytes verbatim instead of inferring a range from the timing.
- `week = 0` means "week not specified" in EDID, so the manufacture date is
  reported as year-only. It is not zero-filled into "week 0".

## 4. Why the instance join must be exact

Registry `HKLM\SYSTEM\CurrentControlSet\Enum\DISPLAY` on the target:

| class | instance | `Device Parameters\EDID` | decode |
| --- | --- | --- | --- |
| `LEN8AD7` | `4&3ab355bd&1&UID8388688` | 384 B | active embedded panel (above) |
| `CSW146D` | `4&3ab355bd&1&UID8388688` | 384 B | manufacturer `CSW`, product `0x146D`, names `CSOT T3` / `MNE507ZA2-4`, **same** 3072 × 1920 @ 60 Hz timing and `0xFD` limits |
| `DELD0E6` | `1&28a6823a&2&UID256` | 128 B | Dell `S2719DGF`, 2560 × 1440 @ 144 Hz, serial text `7JQXBY2`, no `WmiMonitorConnectionParams` instance |
| `Default_Monitor` | `1&8713bca&0&UID0` | no value | generic class record |

`CSW146D` and `LEN8AD7` share the *same instance suffix* while carrying different
class EDIDs for the same physical panel, so "read the first `DISPLAY` record" or
"match on the instance suffix" would both be non-deterministic. The
implementation joins on the full device instance path returned by
`WmiMonitorConnectionParams` (class included), and additionally refuses the block
when the class component does not equal the EDID's manufacturer + product code
(`LEN8AD7` vs `LEN`/`0x8AD7`). `DELD0E6` demonstrates the other trap: a perfectly
valid EDID for a monitor that is not attached.

## 5. Refusals

Enforced by `lctrl_core::decode_panel_edid` before any field is reported:

| defect | behavior | evidence |
| --- | --- | --- |
| length not a whole number of 128-byte blocks | refused | unit test (synthetic truncation of the captured value to 131 bytes) |
| any block checksum ≠ 0 | refused, names the block index | unit test (one flipped byte) |
| base block header ≠ `00 FF FF FF FF FF FF 00` | refused | unit test (header byte changed, checksum resealed) |
| declared extension count ≠ blocks read | refused | unit test (value truncated to 2 of 3 blocks) |
| preferred DTD with a zero active/blank field | refused | unit test (vertical active zeroed, checksum resealed) |
| base block version major ≠ 1 | refused | code path, no sample |

Selection refusals (`lctrl-hal-win::panel_identity`): zero internal connections →
`Unsupported`; two or more → `ChannelUnavailable` (ambiguous, never "pick one");
`InstanceName` without a numeric `_N` index → `ChannelUnavailable`; class/EDID
mismatch → `ChannelUnavailable`.

Also refused at read time: a missing `Device Parameters\EDID` value
(`Default_Monitor` above), a value that is not `REG_BINARY`, an empty value, and a
value above 32 KiB.

## 6. Implementation map

| layer | path / symbol |
| --- | --- |
| decoder + types | `crates/lctrl-core/src/edid.rs`: `decode_panel_edid`, `PanelIdentity`, `PanelConnection`, `PanelInput`, `PanelResolution`, `PanelPhysicalSize`, `PanelManufacture`, `PanelEdidVersion`, `PanelRangeLimits`; re-exported from `lctrl-core` |
| HAL trait | `crates/lctrl-hal/src/peripherals.rs`: `PanelIdentityControl::identity` |
| Windows channel | `crates/lctrl-hal-win/src/panel_identity.rs`: `WindowsPanelIdentity<W, R>`, `NativePanelRegistry` (`RegOpenKeyExW`/`RegQueryValueExW`), `internal_panel_device` probe |
| capability row | `CommandId::PANEL_INFO` (`panel.info`), `ServiceRole::PanelIdentity`, `FeatureBinding::Capability("panel.identity")`, dependencies `PublicWin32Cim` + `Win32Registry`, `DispatchSupport::Implemented` |
| capability probe | `windows_hal` records `channel.panel-identity` and `panel.identity` from the `WmiMonitorConnectionParams` selection only |
| CLI | `PanelCommand::Info` (`panel info`), `CommandServices::with_panel_identity`, `execute_panel_info`; composition in `vantage/src/main.rs` |

`panel info` is read-only: no state layer is written, so the capability row is
`CommandAxes::read_only(StateLayer::Runtime)` with no rollback.

## 7. What this note does not prove

- No build, no run, no screenshot. Expected output is derived from the decoded
  bytes, not from an executed `vantage panel info`.
- LVDS is **not** EDID-derivable. EDID has no LVDS interface code; LVDS panels
  present a digital input whose interface nibble is `0`, which is also what many
  eDP panels with an EDID 1.3-style digital byte report. For an internal panel
  with interface `0` the implementation reports `connection: unspecified` plus
  the raw `interface_code` and says so in the human output. This SKU is eDP
  (`interface 5` + `VOT_INTERNAL`) — that branch is proven; the LVDS branch is
  deliberately not claimed.
- The capability probe only proves the *selection* channel. A machine that passes
  the probe but has a missing or corrupt EDID value fails at command time with
  the precise reason.
- `WmiMonitorID` / `WmiMonitorBasicDisplayParams` are empty through this
  transport, so vendor-name and active-flag cross-checks are unavailable here.

## 8. Parent acceptance steps (not run by this work)

1. Build per the shared contract (`cargo check -p lctrl-hal-win --target
   x86_64-pc-windows-msvc`, then the target `.bat` build) and run the new unit
   tests:
   - `lctrl-core`: `edid::tests` (reference decode, external-DP decode, and the
     five malformed-input refusals).
   - `lctrl-hal-win`: `panel_identity::tests` (selection, signed/unsigned
     `VOT_INTERNAL`, ambiguity, missing index, class cross-check).
2. On the target: `vantage panel info` and `vantage panel info --json`. Expect
   `panel: LEN8AD7 (LEN14.53K)`, `manufactured: 2025 (week not specified)`,
   `native resolution: 3072x1920 @ 60Hz (preferred timing)`,
   `physical size: 310x200 mm (14.5 in diagonal)`,
   `input: digital, 10 bits per primary, interface code 5`,
   `connection: edp (internal panel)`,
   `limits: 30-120 Hz vertical, 240-240 kHz horizontal, <= 780 MHz pixel clock`.
3. `vantage capability --json` must list `channel.panel-identity` and
   `panel.identity` as available on this machine, and `panel.info` as
   implemented.
4. Negative controls available without writing anything: the same command over the
   SSH logon (it must work, because the channel is session-independent — unlike
   `panel rate`), and the Linux build, where no panel service is attached and
   `panel info` must report the structured `Unsupported` error (exit code 3).
