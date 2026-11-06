# Touchpad capability descriptor — recon and `touchpad caps` contract — 2026-11-06

Target: `TB14P-EZRA` / LENOVO 21VG (ThinkBook 14 G8+ IPH), Windows 11
Professional for Workstations, build 26300, firmware `SWCN22WW`.
Scope: the read-only capability read (`touchpad caps`). The switch
(`touchpad on|off`), the aggregate read (`touchpad status`) and the repair verb
(`touchpad recover`) belong to the separate slice documented in
`evidence/re-touchpad-20261105.md`; nothing here writes a device, a registry
value or a setting.

## 1. Sources that do not carry the capability descriptor

Both were checked on the target on 2026-11-06 before touching the HID plane.

- **`root\WMI` has no touchpad class.** `Get-CimClass -Namespace root\wmi`
  enumerates 1650 classes on this machine. Filtering on
  `Touch|Precision|Digitiz|Hid|Ptp` returns **zero** classes
  (`HID_DEVICE_UP:000D_U:0005` is a device-interface hardware ID, not a WMI
  class). The "root\WMI PrecisionTouchpad" lead in the work item is dead on this
  SKU; there is no WMI capability object to read.
- **The PrecisionTouchPad registry trees hold settings, not capabilities.**
  `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\PrecisionTouchPad` contains
  exactly one value, `Status\Enabled = 1`. `HKLM\...\PrecisionTouchPad` contains
  the bookkeeping keys `IgnoredExternalMice`, `LegacyControlled`,
  `LegacyDevices` and no values. `HKLM\SOFTWARE\Microsoft\PrecisionTouchPad`
  does not exist. Nothing here reports contacts, button type or surface size.
- The ACPI parent's `Device Parameters` key under
  `HKLM\SYSTEM\CurrentControlSet\Enum\ACPI\GXTP5100\<instance>` carries only driver policy
  (`DeviceResetNotificationEnabled=1`, `EnhancedPowerManagementEnabled=1`,
  `LegacyTouchScaling=0`, `FirmwareIdentified=1`), no descriptor blob.
- The raw report descriptor is **not needed**. `DeviceIoControl` attempts with
  `IOCTL_HID_GET_DEVICE_ATTRIBUTES` (0x000B0013) and
  `IOCTL_HID_GET_REPORT_DESCRIPTOR` (0x000B0007) on the configuration and
  vendor collections returned `ERROR_INVALID_FUNCTION` (1) in this probe — the
  first attempt used wrong codes (0x000B0190/0x000B0008) and is discarded, so
  the remaining failure is a device-specific refusal, not proof about the IOCTL
  contract. The preparsed data below is what the HID class driver already
  parsed, so the raw descriptor stays unnecessary and that question is left
  open.

## 2. The channel that does carry it

The capability descriptor is the **HID preparsed data of the precision-touchpad
top-level collection**, `HID\GXTP5100&Col02\5&43d3fe9&0&0001`
(`HID_DEVICE_UP:000D_U:0005`), read through the documented user-mode HID APIs:

| Call | Purpose |
|---|---|
| `CM_Get_Device_Interface_ListW(GUID_DEVINTERFACE_HID)` | locate the collection's interface symbolic link for the admitted devnode |
| `CreateFileW(link, FILE_READ_ATTRIBUTES, FILE_SHARE_READ\|FILE_SHARE_WRITE)` | open the collection with the only access mask this SKU accepts |
| `HidD_GetAttributes` | vendor id, product id, version |
| `HidD_GetProductString` | device product string |
| `HidD_GetPreparsedData` | parsed report descriptor |
| `HidP_GetCaps` | top-level usage, report lengths, link-collection and cap counts |
| `HidP_GetValueCaps(HidP_Input / HidP_Feature)` | per-usage logical/physical ranges, units, bit sizes, report ids |
| `HidP_GetButtonCaps(HidP_Input)` | per-contact Tip/Confidence declarations and the integrated button |
| `HidD_GetFeature(report 2)` + `HidP_GetUsageValue(HidP_Feature, …)` | the declared values of Contact Count Maximum and Button Type |

### 2.1 Open semantics are the crux

The Windows input stack holds the collection, so the access mask decides whether
anything can be read at all (measured 2026-11-06, elevated shell):

| dwDesiredAccess | dwShareMode | Result |
|---|---|---|
| `GENERIC_READ` (0x80000000) | READ\|WRITE (3) | **`ERROR_SHARING_VIOLATION` (32)** |
| `GENERIC_READ` (0x80000000) | 0 | `ERROR_SHARING_VIOLATION` (32) |
| `FILE_READ_DATA` (0x00000001) | READ\|WRITE (3) | `ERROR_SHARING_VIOLATION` (32) |
| `FILE_READ_ATTRIBUTES` (0x00000080) | READ\|WRITE (3) | **opens**; `HidD_GetPreparsedData` and `HidD_GetFeature` both succeed |
| 0 (no access) | READ\|WRITE (3) | opens; the same two calls succeed |

The sibling mouse collection `HID\GXTP5100&Col01` answers `GENERIC_READ` with
`ERROR_ACCESS_DENIED` (5) — it is a system mouse collection. The
implementation therefore uses **`FILE_READ_ATTRIBUTES` with share READ|WRITE and
nothing else**, and when that open fails it reports `ChannelUnavailable` with the
Win32 error instead of retrying a broader access mask. Broadening the request is
what this machine rejects; silently falling back would turn a refusal into a
fabricated success or an operator-visible sharing error on an unrelated command.

## 3. Verbatim receipts

### 3.1 Open-mode matrix (probe 6)

```text
OPEN GENERIC_READ/share3 -> err=32
OPEN GENERIC_READ/share0 -> err=32
OPEN FILE_READ_DATA/share3 -> err=32
OPEN FILE_READ_ATTRIBUTES/share3 -> ok
  GetFeature id=2 ok first8=0215000000000000 len=737
  UsageValue contactCountMax status=0x00110000 value=5
  UsageValue buttonType status=0x00110000 value=1
OPEN no-access/share3 -> ok
  GetFeature id=2 ok first8=0215000000000000 len=737
  UsageValue contactCountMax status=0x00110000 value=5
  UsageValue buttonType status=0x00110000 value=1
```

### 3.2 Capability descriptor of the PTP collection (probe 8)

`SIZES value=72 button=72 caps=64` is the marshalled `HIDP_VALUE_CAPS`,
`HIDP_BUTTON_CAPS` and `HIDP_CAPS` sizes the probe asserted before decoding.

```text
### col02 PTP
caps st=0x00110000 page=0x000D usage=0x0005 in=40 out=0 feat=737 links=7 inBtn=11 inVal=22 featBtn=1 featVal=8
in valueCaps n=22 st=0x00110000
  page=0x000D usage 0x0051 rid=4 lcoll=1 luse=0x0022/0x000D bits=4 cnt=1 lmin=0 lmax=15 pmin=0 pmax=0 unit=0x00 exp=0
  page=0x0001 usage 0x0030 rid=4 lcoll=1 luse=0x0022/0x000D bits=16 cnt=1 lmin=0 lmax=4149 pmin=0 pmax=519 unit=0x13 exp=14
  page=0x0001 usage 0x0031 rid=4 lcoll=1 luse=0x0022/0x000D bits=16 cnt=1 lmin=0 lmax=2147 pmin=0 pmax=302 unit=0x13 exp=14
  page=0x000D usage 0x0030 rid=4 lcoll=1 luse=0x0022/0x000D bits=16 cnt=1 lmin=0 lmax=2000 pmin=0 pmax=2000 unit=0x21 exp=14
  ... same four usages repeated for link collections 2..5 (Contact ID / X / Y / Pressure) ...
  page=0x000D usage 0x0056 rid=4 lcoll=0 luse=0x0005/0x000D bits=16 cnt=1 lmin=0 lmax=65535 pmin=0 pmax=65535 unit=0x1001 exp=12
  page=0x000D usage 0x0054 rid=4 lcoll=0 luse=0x0005/0x000D bits=8 cnt=1 lmin=0 lmax=127 pmin=0 pmax=65535 unit=0x1001 exp=12
in buttonCaps n=11 st=0x00110000
  page=0x000D usage 0x0042 rid=4 lcoll=1..5 luse=0x0022/0x000D bitfield=0x0002 cnt=1     (Tip switch, one per finger)
  page=0x000D usage 0x0047 rid=4 lcoll=1..5 luse=0x0022/0x000D bitfield=0x0002 cnt=1     (Confidence, one per finger)
  page=0x0009 usage 0x0001 rid=4 lcoll=0    luse=0x0005/0x000D bitfield=0x0002 cnt=1     (Button 1, top level)
feat valueCaps n=8 st=0x00110000
  page=0x000D usage 0x0059 rid=2 lcoll=0 luse=0x0005/0x000D bits=4 cnt=1 lmin=0 lmax=15 pmin=0 pmax=65535 unit=0x1001 exp=12
  page=0x000D usage 0x0055 rid=2 lcoll=0 luse=0x0005/0x000D bits=4 cnt=1 lmin=0 lmax=15 pmin=0 pmax=65535 unit=0x1001 exp=12
  page=0x000E usage 0x0023 rid=9 lcoll=6 luse=0x0001/0x000E bits=8 cnt=1 lmin=0 lmax=100 pmin=0 pmax=65535 unit=0x1001 exp=12
  page=0x000D usage 0x00B0 rid=8 lcoll=0 luse=0x0005/0x000D bits=2 cnt=1 lmin=1 lmax=3 pmin=110 pmax=190 unit=0x101 exp=12
  page=0xFF00 usage 0x00C5 rid=6 lcoll=0 luse=0x0005/0x000D bits=8 cnt=256 lmin=0 lmax=255 ...
  page=0xFF00 usage 0x00C4 rid=13 lcoll=0 luse=0x0005/0x000D bits=8 cnt=4 ...
  page=0xFF00 usage 0x00C6 rid=12 lcoll=0 luse=0x0005/0x000D bits=8 cnt=736 ...
  page=0xFF00 usage 0x00C7 rid=11 lcoll=0 luse=0x0005/0x000D bits=8 cnt=66 ...
feat buttonCaps n=1 st=0x00110000
  page=0x000D usage 0x0060 rid=7 lcoll=0 luse=0x0005/0x000D bitfield=0x0002 cnt=1     (Latency Mode)
```

Identity from the same handle: `vid=0x27C6 pid=0x01E7 ver=0xFF03 product='HIDI2C Device'`.

### 3.3 The sister collections are not a capability source (probe 8)

```text
### col03 CFG
caps page=0x000D usage=0x000E in=0 out=0 feat=3 links=3 inBtn=0 inVal=0 featBtn=2 featVal=1
feat valueCaps n=1:  page=0x000D usage 0x0052 (Input Mode) bits=8 lmin=0 lmax=10
feat buttonCaps n=2: page=0x000D usage 0x0058 (Button switch), page=0x000D usage 0x0057 (Surface switch)
### col04 VND
caps page=0xFF00 usage=0x0001 in=65 out=65 feat=0
```

The configuration collection (`0x000D/0x000E`, MTConfig) exposes Input Mode and
the Surface/Button switches only; it does **not** expose Contact Count Maximum or
Button Type. The vendor collection is the Goodix firmware tunnel, out of scope
for this verb. Both facts are negative controls: a capability read must refuse a
collection whose top-level usage is not `0x000D/0x0005`.

## 4. Decoded contract

Usage names/values are taken from Microsoft's Windows Precision Touchpad
collection guide and the sample report descriptors (same usage ids, bit sizes and
ranges), not inferred from the device shape.

| Fact | Declared by | Value on 21VG |
|---|---|---|
| top-level collection | `HidP_GetCaps` | page `0x000D`, usage `0x0005` (Touch Pad) |
| report lengths | `HidP_GetCaps` | input 40 B, output 0 B, feature 737 B |
| link collections | `HidP_GetCaps` | 7 (1 top level + 5 finger + 1 Simple Haptics Controller) |
| concurrent contacts declared | `0x0D/0x22` Finger link collections in the input report | **5** |
| Contact Count Maximum | feature `0x0D/0x55`, report 2, read back | **5** |
| Button Type | feature `0x0D/0x59`, report 2, read back | **1 = Non-Depressible (Pressure-pad)** |
| Contact Identifier | `0x0D/0x51`, 4 bit | logical `0..15` |
| Contact Count | `0x0D/0x54`, 8 bit | logical `0..127` |
| Scan Time | `0x0D/0x56`, 16 bit | logical `0..65535` |
| X | `0x01/0x30` | logical `0..4149`, physical `0..519`, unit `0x0013`, exponent `-2` |
| Y | `0x01/0x31` | logical `0..2147`, physical `0..302`, unit `0x0013`, exponent `-2` |
| pressure | `0x0D/0x30` per finger | logical `0..2000`, physical `0..2000`, unit `0x0021`, exponent `-2` |
| Tip switch / Confidence | `0x0D/0x42`, `0x0D/0x47` per finger | declared, 1 bit each |
| integrated button | `0x09/0x01` top level | declared |
| haptic intensity | feature `0x0E/0x23` in `0x0E/0x01` Simple Haptics Controller | declared, report 9, logical `0..100` |
| button press threshold | feature `0x0D/0x0B0` | declared, report 8, logical `1..3`, physical `110..190` |
| latency mode | feature `0x0D/0x60` | declared, report 7 |

### 4.1 Surface size is derived, not reported

`unit 0x0013` is `UNIT(Inch, EngLinear)` and exponent `-2`, i.e. hundredths of an
inch. 519 × 0.01 in = 5.19 in = **131.8 mm**; 302 × 0.01 in = 3.02 in =
**76.7 mm**. This is arithmetic over the declared physical range: it is labelled
`derived_size_mm` in the output and must never be presented as a measured
surface. Width ⨯ height is consistent with a 1.72 ratio, but the number is only
as good as the firmware's Physical Maximum.

## 5. Implemented contract

`vantage touchpad caps` (read-only; no `--yes` needed and no write path) and the
JSON form below. Every field is what the *device declares*.

```json
{
  "instance_id": "HID\\GXTP5100&COL02\\5&43d3fe9&0&0001",
  "usage": { "page": 13, "usage": 5 },
  "identity": { "vendor_id": 10182, "product_id": 487, "version": 65283, "product": "HIDI2C Device" },
  "reports": { "input_bytes": 40, "output_bytes": 0, "feature_bytes": 737, "link_collections": 7 },
  "contacts": { "declared_collections": 5, "identifier_max": 15, "count_field_max": 127, "count_maximum": 5 },
  "surface": {
    "x": { "logical_min": 0, "logical_max": 4149, "physical_min": 0, "physical_max": 519, "unit": 19, "unit_exponent": -2 },
    "y": { "logical_min": 0, "logical_max": 2147, "physical_min": 0, "physical_max": 302, "unit": 19, "unit_exponent": -2 },
    "derived_size_mm": { "width_mm": 131.8, "height_mm": 76.7 }
  },
  "buttons": { "button_type": "non_depressible", "integrated_button": true },
  "support": {
    "two_finger": true, "three_finger": true, "four_finger": true, "five_finger": true,
    "pressure": true, "palm_confidence": true, "haptic_intensity": true, "button_press_threshold": true
  }
}
```

Derivation rules (pure function, unit-tested without a device):

1. **Refuse a non-PTP collection.** A top-level usage other than
   `0x000D/0x0005` is `Unsupported`; the configuration (`0x000D/0x000E`) and
   vendor (`0xFF00/0x0001`) collections are never decoded as a touchpad.
2. **Refuse an incomplete mandatory set.** No Finger link collection, no X/Y in
   those collections, no Contact Identifier, no Contact Count field, or a
   truncated cap (`bit_size == 0`, `logical_max <= logical_min`, disagreeing
   per-contact X/Y ranges) is `ChannelUnavailable` naming the usage — nothing is
   defaulted or invented.
3. `contacts.declared_collections` counts distinct Finger link collections in the
   input report; `contacts.count_maximum` is the feature-report value when report
   2 could be read, otherwise `null` plus `capability_report_error`.
4. `buttons.button_type` maps `0 → depressible`, `1 → non_depressible`,
   `2 → non_clickable`, other values → `{ "other": n }`; absent when the report
   could not be read.
5. `support.two_finger … five_finger` mean "the collection declares at least N
   concurrent contacts"; `pressure`, `palm_confidence` (Confidence usage),
   `haptic_intensity` and `button_press_threshold` mirror declared usages.
   Windows synthesizes the actual gestures from those contacts, so these bits are
   device capability, not a statement that a Windows gesture is enabled.
6. `derived_size_mm` exists only for `unit 0x0013` with a physical span > 0 and
   `|exponent| <= 6`; every other unit leaves it absent.

## 6. Limits and non-claims

- **Declared ≠ guaranteed.** `Contact Count Maximum = 5` and `Button Type = 1`
  are the values the device capabilities feature report returned on 2026-11-06.
  They do not prove the surface reports five independent contacts, nor that
  press/haptic hardware works. The Goodix MCU can hang while the descriptor
  stays readable (see `evidence/re-touchpad-20261105.md` §3).
- Reading the descriptor does not exercise the device: `HidD_GetFeature(report 2)`
  is a read of the capability report, no `SET_FEATURE`, no output report, no PnP
  write. `touchpad caps` therefore never changes state and needs no dry-run.
- The open-mode observation was made in an **elevated** shell. A non-elevated
  process is not covered by this receipt; if `FILE_READ_ATTRIBUTES` is refused
  there, the command reports `ChannelUnavailable` with the Win32 error instead of
  guessing.
- The one Open item: `docs/04-peripherals.md` needs the `caps` verb added to its
  command table and section; that document is shared with the switch/recover
  slice and was not rewritten here.

## 7. Acceptance that was NOT run

Source-only change in a shared worktree: no build, test, lint, or CLI execution
was performed by this slice. Parent-owned checks:

1. `cargo check -p lctrl-hal-win --target x86_64-pc-windows-msvc` and the target
   `.bat` build.
2. On the target: `vantage --json touchpad caps` must print exactly the values in
   §5 and exit 0; `vantage touchpad caps` (human) must label the millimetres line
   as derived.
3. Negative control: the same command on a machine without a PTP collection must
   fail with `Unsupported`, and `touchpad caps` must never appear in the
   mutation set (`command_is_mutating`) or require `--yes`.
4. Negative control: the capability row for `touchpad.caps` must show
   `read_only`/`Runtime` axes and `Implemented` dispatch.
5. Re-run the open-mode matrix (§2.1) if a future Windows build changes the
   sharing behaviour; the failure mode is `ChannelUnavailable`, never a silent
   fallback.
