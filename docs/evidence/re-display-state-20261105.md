# Display state RE — HDR, refresh rate, DPI scaling, internal/external (21VG, 2026-11-05/06)

Slice: `DISPLAY-STATE` (`vantage display status`, read-only).
Target: `tb14p-ezra` = ThinkBook 14 G8+ IPH (`21VG`), Windows 11 build 26300
(`10.0.26300`), Intel Arc B390 (`PCI\VEN_8086&DEV_B080`), BIOS SWCN22WW.
Collected **read-only**: CCD/DPI/colour APIs called for reads only, `Get-CimInstance`,
`RegQueryValueEx`-equivalent registry reads, plus static analysis of Lenovo addins
copied off the target and of `dxgkrnl.sys` on disk. Nothing was written, installed,
started, or reconfigured; no HDR, scaling, or mode transition was requested; no
vendor binary was executed.

Question: how does this SKU expose HDR (including DisplayHDR), the current refresh
rate, the scaling percentage, and whether a display is the internal panel — and which
of those surfaces is readable without an interactive desktop session?

---

## 1. The decisive session fact

The SSH logon is a **session 0** logon. Measured live:

| probe | result |
|---|---|
| `Process.GetCurrentProcess().SessionId` / `[Environment]::UserInteractive` | `0` / `False` |
| `quser` | `ezra  console  1  Active` — the interactive session exists, it is just not ours |
| `GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS)` | `0` (sizes returned) |
| `QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS)` | **`5` = `ERROR_ACCESS_DENIED`** |
| `EnumDisplayMonitors` | one monitor: device `WinDisc`, bounds `0,0..1024,768`, `dpi=96x96` |
| `System.Windows.Forms.Screen.PrimaryScreen` | `WinDisc 1024x768` |

So the entire CCD/DPI/colour API family answers for the *calling session's* desktop —
here a synthetic 1024×768 `WinDisc` — and `QueryDisplayConfig` is refused outright.
This is the same class of restriction already recorded for the MSR device
(`docs/evidence/wmi-capability-exhaustion-20261104.md` §10): a network logon cannot
observe or drive the console session's display.

Consequence: a `display status` built only on `QueryDisplayConfig` would either fail
or report `1024x768 @ 96 dpi` as "the panel" from SSH. Both are wrong answers, so the
command has **two explicitly attributed scopes**:

- `displays` — live state of the calling session's display paths (authoritative when
  run on the desktop; reports the refusal verbatim otherwise);
- `panels` — per-monitor colour/HDR state that the display driver keeps
  machine-wide, joined to the connected monitor by PnP hardware id. Readable from
  session 0 (verified below).

`session_id`, `console_session_id` and `interactive` are always reported so a consumer
can tell which scope a number came from without parsing prose.

---

## 2. Field → authoritative source (live)

### 2.1 Machine-wide (readable from session 0)

| Field | Source | Live value on 21VG |
|---|---|---|
| embedded panel present / internal | `root\wmi:WmiMonitorConnectionParams.VideoOutputTechnology` | one instance, `DISPLAY\LEN8AD7\4&3ab355bd&1&UID8388688_0`, `0x80000000` = `D3DKMDT_VOT_INTERNAL` |
| monitor identity / native mode | `HKLM\SYSTEM\CurrentControlSet\Enum\DISPLAY\<mfg>\<instance>\Device Parameters\EDID` | `LEN8AD7`, name `LEN14.53K`, 3072×1920 preferred, 31×20 cm |
| HDR capability declared by the sink | EDID CTA-861 extension, HDR Static Metadata data block (extended tag `0x06`) | `e6 06 05 01 6a 6a 48` → EOTF `0x05`, SMD `0x01`, codes `106/106/72` |
| HDR enabled / SDR white / ACM state | `HKLM\SYSTEM\CurrentControlSet\Control\GraphicsDrivers\MonitorDataStore\<monitor unique name>` | `HDREnabled=0`, `SDRWhiteLevel=6000`, `AutoColorManagementSupported=1`, `AutoColorManagementEnabled=0`, `DisplayHdrLevelNotified=1` |
| firmware refresh table | `root\wmi:LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA` | 22 identical rows (`GMZN_0..21`): `Min=60 Max=120 Default=60 Mode=1` |
| adapter's current mode | `root\CIMV2:Win32_VideoController` (`CurrentRefreshRate`, `CurrentHorizontalResolution`, …) | Intel Arc B390: `3072x1920 @ 120 Hz, 32 bpp` |

The EDID HDR block decodes with the CTA-861.3 bit layout (bit 0 = SDR gamma, bit 1 =
traditional gamma HDR range, **bit 2 = SMPTE ST 2084 / HDR10 PQ**, bit 3 = HLG; the
same numbering Linux pins in `enum hdmi_eotf` and tests with `BIT()`):

```text
EOTF 0x05 = SDR + ST 2084          → the panel declares HDR10 (PQ), no HLG
SMD  0x01  = static metadata type 1
max = 50·2^(106/32) ≈ 496.7 nits, avg ≈ 496.7 nits
min = max·(72/255)²/100 ≈ 0.396 nits
```

Corroboration that this is *not* a mis-read of an SDR panel: the same EDID's
Colorimetry block (`e3 05 80 80`, extended tag `0x05`) sets BT.2020 RGB, the range-limits
descriptor (`fd 00 1e 78 f0 f0 4e 01`) bounds 30–120 Hz / 240–240 kHz / 780 MHz, and the
driver store carries `DisplayHdrLevelNotified=1` with `SDRWhiteLevel=6000` (= 6000/1000·80
= **480 nits** by the documented `DISPLAYCONFIG_SDR_WHITE_LEVEL` unit, i.e. inside the
panel's own 497-nit ceiling).

The same EDID shape is present on the second 3K panel record for the same instance
suffix (`CSW146D`, `MNE507ZA2-4`), which is why the panel-identity channel refuses to
pick a `DISPLAY` record by suffix alone.

### 2.2 Session-scoped (needs desktop access)

| Field | Source | Readable from SSH? |
|---|---|---|
| active paths, internal/external, rotation, target refresh rational | `QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS)` + `DISPLAYCONFIG_TARGET_DEVICE_NAME` | no (`ERROR_ACCESS_DENIED`) |
| current desktop mode (pixels, bpp, Hz, position) | `EnumDisplaySettingsExW(ENUM_CURRENT_SETTINGS)` per GDI source name | no (only `WinDisc`) |
| HDR supported / HDR user-enabled / active colour mode | `DisplayConfigGetDeviceInfo(GET_ADVANCED_COLOR_INFO_2 = 15)` | no |
| HDR-or-WCG "advanced colour" bits | `DisplayConfigGetDeviceInfo(GET_ADVANCED_COLOR_INFO = 9)` | no |
| SDR white level | `DisplayConfigGetDeviceInfo(GET_SDR_WHITE_LEVEL = 11)` | no |
| scaling | `EnumDisplayMonitors` + `GetMonitorInfoW` + `GetDpiForMonitor(MDT_EFFECTIVE_DPI)` | no (96 dpi for `WinDisc`) |

---

## 3. Session-scoped contract details

### 3.1 HDR is split on 24H2, and the legacy type cannot answer it

Windows 11 24H2 (build 26100) added a second advanced-colour request; the numeric
values come from mingw-w64 `wingdi.h` (September 2024 commit `e33940a2`) and the
structure layouts are mirrored in Kodi's `SDK_26100.h` shim (`xbmc/xbmc#26096`):

```text
 9  GET_ADVANCED_COLOR_INFO       32-byte struct:
      union value bits: 0 advancedColorSupported, 1 advancedColorEnabled,
                        2 wideColorEnforced, 3 advancedColorForceDisabled
      colorEncoding, bitsPerColorChannel
11  GET_SDR_WHITE_LEVEL           24-byte struct; SDRWhiteLevel = multiplier of 80 nits × 1000
15  GET_ADVANCED_COLOR_INFO_2     36-byte struct:
      union value bits: 0 advancedColorSupported, 1 advancedColorActive, 2 reserved,
                        3 advancedColorLimitedByPolicy, 4 highDynamicRangeSupported,
                        5 highDynamicRangeUserEnabled, 6 wideColorSupported,
                        7 wideColorUserEnabled
      colorEncoding, bitsPerColorChannel, activeColorMode  (0 SDR, 1 WCG, 2 HDR)
16  SET_HDR_STATE, 17 SET_WCG_STATE                 (write side — not used here)
```

`advancedColorSupported` (type 9, bit 0) means "HDR **or** WCG", so a type-9-only read
cannot decide HDR support, and `advancedColorEnabled` is set for WCG too. The command
therefore reads type 15 first and reports `hdr_supported` / `hdr_user_enabled` from bits
4/5 and `active_mode` from `activeColorMode`; only if the OS rejects type 15 does it fall
back to type 9, and then it reports the advanced-colour bits with `hdr_supported: null`
rather than translating them into an HDR claim.

Live verification on this target was **not possible**: session 0 has zero active paths,
so there is no target handle to query. The type-15 path is contract-derived and is
exercised only when `display status` runs inside the desktop session (§7).

### 3.2 Scaling is per-monitor DPI, and needs a DPI-aware process

`GetDpiForMonitor(MDT_EFFECTIVE_DPI)` returns the scaling actually applied to a monitor;
the percentage is `dpi / 96 × 100`. Two preconditions are easy to miss:

- the caller must be per-monitor DPI aware (`SetProcessDpiAwarenessContext(
  PER_MONITOR_AWARE_V2)`); an unaware process is virtualised to 96 dpi for every
  monitor. The probe shows the call failing (`0`) when issued after user32 was already
  touched, and `GetDpiForSystem() == 96` in session 0;
- `GetDpiForMonitor` takes an `HMONITOR`, not the GDI device name, so the monitor list
  (`EnumDisplayMonitors` + `GetMonitorInfoW.szDevice`) is the join between the CCD path's
  GDI name (`\\.\DISPLAY1`) and its DPI.

The Windows-internal relative form (`ScaleFactors\<set id>` `DpiValue`, observed
`0xFFFFFFFE` = −2 for the internal panel) is **relative to the OS-recommended scaling**
("move N steps in 100/125/150/175/200/225/250/300/350/400/450/500 from the recommended
value"), so a raw `DpiValue` cannot be converted to a percentage without the
recommended value. It is therefore not used for the reported percentage; it is only
noted here as the persisted form.

### 3.3 Internal vs external

`DISPLAYCONFIG_PATH_TARGET_INFO.outputTechnology` is the classification; the embedded
codes are `DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL` (`0x80000000`), `LVDS`, 
`DISPLAYPORT_EMBEDDED`, `UDI_EMBEDDED`. `QueryDisplayConfig` is session-bound, but the
WMI equivalent is not: `WmiMonitorConnectionParams` reports the same
`D3DKMDT_VIDEO_OUTPUT_TECHNOLOGY` per connected monitor and was readable from session 0
(one instance, internal).

Two **negative** facts matter here, because both would otherwise be read as displays:

- `Win32_VideoController` lists three adapters on this machine — Intel Arc B390 plus
  `GameViewer Virtual Display Adapter` (向日葵 remote desktop) and `OrayIddDriver`.
  Both virtual adapters report a "current mode" (3072×1920, one of them at 144 Hz), so
  an adapter-mode read is not a panel read and must not be attributed to a monitor.
- `WmiMonitorConnectionParams` / `WmiMonitorDescriptorMethods` enumerate only the
  physical panel, so the virtual adapters never appear as monitors.

---

## 4. Driver / registry contract (machine-wide HDR state)

`MonitorDataStore` is not a vendor invention: the value names are literals in
`C:\Windows\System32\drivers\dxgkrnl.sys` (5,320,072 bytes), next to the key path
itself:

```text
\Registry\Machine\System\CurrentControlSet\Control\GraphicsDrivers\MonitorDataStore
CurrentControlSet\Control\GraphicsDrivers\ScaleFactors
HDREnabled                     IsHDREnabled
SDRWhiteLevel                  DefaultIntegratedSdrWhiteLevel / DefaultExternalSdrWhiteLevel
AutoColorManagementSupported   AutoColorManagementEnabled
DisplayHdrLevelNotified        ScaleFactorsFromRemoteClient / HeuristicScaleFactors
MonitorGetIsAutoColorManagementSupported() / MonitorSetSDRWhiteLevel() / MonitorGetSdrWhiteLevelWnfName
```

So the per-monitor colour state the OS is running with is stored, per monitor unique
name, by the kernel display stack — which is why it is readable without a desktop.
`SDRWhiteLevel` uses the same unit as `DISPLAYCONFIG_SDR_WHITE_LEVEL` (×80/1000 nits);
that equality is **[INFERENCE]** from the value name and the two live numbers
(6000 ↔ 480 nits inside a 497-nit panel), not from a documented statement.

Joining a monitor to its store key: the store key is
`<mfg><product code><serial if nonzero>_<week>_<year>_<byte>`
(`LEN8AD7` + `0_00_07E9_C9`; `DEL` + `D0E6` + `7JQXBY2_24_07E3_2C`). The PnP hardware id
(`DISPLAY\LEN8AD7\...`) is the exact prefix, so the join is a unique prefix match. If
two connected monitors share a hardware id the join is ambiguous and the state is
reported as absent rather than picked arbitrarily.

---

## 5. Vendor contract (what Lenovo owns, and what it does not)

Static analysis of the installed addins (copied off the target, not executed):

| concern | owner | mechanism |
|---|---|---|
| HDR / auto colour management | `C:\ProgramData\Lenovo\Vantage\Addins\SmartPanelAddin\1.0.5.154\ACM.dll` (PE32+ x64, image base `0x180000000`, `TimeDateStamp` 2026-07-09) | exports `GetACMStatus`, `GetHDRStatus`, `GetAbility`, **`GetIs24H2OrLater`**, `GetWhiteListStatus`, `RegisterACMEventCallback`, `SetACMStatus`; imports `DisplayConfigGetDeviceInfo`/`DisplayConfigSetDeviceInfo`/`QueryDisplayConfig`/`GetDisplayConfigBufferSizes`/`EnumDisplayMonitors`/`GetMonitorInfoW`/`EnumDisplaySettingsW`/`EnumDisplayDevicesW` + two `mscms.dll` ordinal imports |
| HDR front-end | `...\SmartPanelAddin.dll` (managed) | contract `Contract_TconColor`: `GetColorManagerState`/`SetColorManagerState`, `AutoColor_HDR`, `ACMHDRRegistryPath`; caches `HDREnabled`/`AdvancedColorEnabled`/`AutoColorManagementEnabled`/`AutoColorManagementSupported` under `HKLM\SOFTWARE\Lenovo\VantageService\AddinData\SmartPanelAddin`; WMI notify method `_Vantage_Tcon_RegNotifyACMMEventChange` |
| refresh rate / adaptive refresh | `...\SmartDisplayAddin\x64\SEAdaptiveRefreshRatePlugin.dll` | `SELECT * FROM LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA` for min/max, falls back to `root\CIMV2 Win32_VideoController.CurrentRefreshRate` **with min hardcoded to 60**, then `QueryDisplayConfig`/`SetDisplayConfig`/`ChangeDisplaySettingsEx`; logs `not internal pannel, can't set refresh rate.` |

`ACM.dll`'s CCD read is recoverable from its own code. At `0x180014ed0` it loops over
targets, fills `DISPLAYCONFIG_TARGET_DEVICE_NAME` (`type = 2`, `size = 0x1A4`), matches
the friendly name, then sets `type = 9` with `size = 0x20` and calls
`DisplayConfigGetDeviceInfo`; on success it stores **`value & 1`** (bit 0,
`advancedColorSupported`) and then consults a whitelist
(`0x180014fab` → `0x180012800`, the `GetWhiteListStatus` path) before publishing the
"AutoColor_HDR" state. The 24H2 gate is an explicit export
(`GetIs24H2OrLater`), which is the vendor's own admission that the HDR-bit semantics
changed in 24H2 — the same split documented in §3.1. Its `GetHDRStatus` export is
`0x1800272f0`, which re-queries the same way for a cached manager field.

Consequences for the replacement:

1. the vendor does **not** use a Lenovo-private HDR transport; it uses the public CCD
   advanced-colour API, so the replacement must too;
2. the vendor's HDR availability is *whitelist-gated* on top of the OS capability bit.
   `GetWhiteListStatus` was not recovered byte-exactly (no on-disk list was found), so
   `display status` reports the OS capability/state only and never claims the vendor's
   whitelist verdict;
3. the firmware/EC refresh table the vendor reads is the same
   `LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA` this repo already uses for `panel rate`,
   and the vendor's own fallback to `Win32_VideoController` proves that this class is
   the session-independent source for the current rate;
4. the "scaling" strings in the same plugin (`ctlGetCurrentScaling`,
   `ctlGetSupportedScalingCapability`, `ctlGetSupportedRetroScalingCapability`,
   `ADL2_DFP_GPUScalingEnable_Get/Set`) are **GPU/retro scaling via IGCL/ADL**, not the
   Windows display scale. No Lenovo addin reads or writes the per-monitor DPI scale
   (`lno.permon.dll` is a SmartWireless component with no DPI surface), so Windows DPI
   scaling has no vendor contract to match.

### 5.1 DisplayHDR tier

The *tier* (DisplayHDR 400/500/…) is not available from WMI or from a documented
registry value:

- `root\wmi` has no advanced-colour class. `WmiMonitorSupportedDisplayFeatures` exists
  but exposes only `ActiveOffSupported`/`DisplayType`/`GTFSupported`/
  `HasPreferredTimingMode`/`sRGBSupported`/`StandbySupported`/`SuspendSupported` — no HDR
  field — and it returns **zero instances** in session 0 (as do `WmiMonitorID`,
  `WmiMonitorBasicDisplayParams`, `WmiMonitorColorCharacteristics`,
  `WmiMonitorListedSupportedSourceModes`), because monitor identity classes are
  session-bound while `WmiMonitorConnectionParams`, `WmiMonitorDescriptorMethods` and
  `WmiMonitorBrightness` still answer;
- the driver store carries only the latch `DisplayHdrLevelNotified = 1`, no level;
- `windows.graphics.dll` carries `Windows.Graphics.Display.AdvancedColorInfo`, i.e. the
  WinRT surface the Settings app uses, and the CCD tier list
  (`DISPLAYHDR_10_400 = 2`, `…500_TRUEBLACK = 64`, `NVIDIA_GSYNC_ULTIMATE = 0x100000`, …)
  is the `DisplayHdrCertifications` field of the **undocumented** `type = -7`
  `DISPLAYCONFIG_GET_MONITOR_INTERNAL_INFO` request; both are session-bound.

So the replacement reports what is verifiable — the EDID-declared EOTF set and
luminance codes, plus the driver's `DisplayHdrLevelNotified` latch — and does not
synthesise a tier.

---

## 6. Live summary of the internal panel

```text
hardware id            LEN8AD7            (mfg LEN, product 0x8AD7)
model / vendor         LEN14.53K / LENOVO 14.53" (31x20 cm), EDID 1.4, week 0 / 2025
connection             internal           WmiMonitorConnectionParams 0x80000000
preferred timing       3072x1920 @ 60 Hz    range limits 30..120 Hz, 240..240 kHz, 780 MHz
HDR capability         SDR + HDR10 (PQ)     BT.2020 RGB, static metadata type 1
                       max/avg 496.7 nits, min 0.396 nits
driver colour state    HDR off, SDR white 480 nits (raw 6000), ACM supported/off,
                       DisplayHdr level notified
firmware refresh       60..120 Hz, default 60, Mode=1
adapter current mode   3072x1920 @ 120 Hz 32 bpp (Intel Arc B390, session-independent)
session 0 view         no paths (QueryDisplayConfig = ERROR_ACCESS_DENIED), WinDisc 1024x768 @ 96 dpi
```

---

## 7. Source changes derived from this evidence

1. `crates/lctrl-core/src/display.rs` (new) — `DisplaySession`, `DisplayConnection`,
   `DisplayScaling`, `DisplayColorState`, `DisplayPathState`, `MonitorColorState`,
   `PanelHdrCapability`, and `decode_panel_hdr_capability` (CTA-861 HDR Static
   Metadata + Colorimetry decode on already-structured EDID bytes). Luminance is
   decoded with integer fixed-point arithmetic (Q20 `2^(r/32)` table) so the reported
   millinits are reproducible and testable, not float-dependent.
2. `crates/lctrl-hal/src/peripherals.rs` — `DisplayStatusControl` with three reads:
   `session()`, `displays()`, `monitors()`.
3. `crates/lctrl-hal-win/src/display_status.rs` (new) —
   `WindowsDisplayStatus<D, A, W, R>`:
   - `D: DisplayApi` provides the session path list and the current mode (the same
     native transport `panel rate` uses);
   - `A: DisplayStateApi` (`NativeDisplayState`) provides type-15/type-9 advanced
     colour, type-11 SDR white level, and per-monitor effective DPI (process DPI
     awareness set once, `PER_MONITOR_AWARE_V2`);
   - `W: WmiTransport` + `R: PanelRegistry` provide the machine-wide half: the internal
     panel instance (`panel_identity::internal_panel_device`), its EDID (HDR
     capability) and the `MonitorDataStore` entries selected by hardware-id prefix.
   Nothing here writes: no `SET_ADVANCED_COLOR_STATE`, no `SET_HDR_STATE`, no
   `ChangeDisplaySettingsEx`, no DPI write, no registry write.
4. `crates/vantage-cli` — `display status` command, capability id `display.status`
   (layer `Runtime`), and a human renderer that names the scope of each number.
5. `crates/lctrl-hal-win/Cargo.toml` — enable the `Win32_UI_HiDpi` and
   `Win32_System_RemoteDesktop` windows-sys features.

## 8. Acceptance that remains unrun, and negative controls

Unrun (needs a desktop session, or is for the parent to schedule):

1. **Read-only live run inside session 1**: `vantage display status` must report at
   least one path, `internal: true`, `mode.refresh_hz` consistent with
   `Win32_VideoController.CurrentRefreshRate` (120 on this machine), `scaling.percent`
   equal to the Settings value, and `color.hdr_supported: true` with
   `hdr_user_enabled` matching the Settings "Use HDR" switch. This is the only check
   that can validate the type-15 bit layout on real hardware.
2. **Session-0 run**: must report `displays` as a permission error (not an empty list,
   not a fake path) while still reporting `panels`.
3. **Negative control — virtual adapters**: with a remote-desktop session open
   (GameViewer/Oray), `display status` must not report a virtual adapter as a monitor;
   `monitors` may only contain PnP monitors from `WmiMonitorConnectionParams`.
4. **Negative control — external monitor**: with the DisplayPort/HDMI sink attached
   (an EDID for a Dell `S2719DGF` is already recorded from an earlier session), the
   external path must report `internal: false` and its own `scaling`, and its EDID must
   yield `hdr_capability: null` (that sink's 128-byte EDID has no CTA extension).
5. **Negative control — WCG-only sink**: per §3.1, a WCG-capable/HDR-incapable target
   must report `hdr_supported: false` even though the legacy type-9 bit 0 is set. Not
   available on this SKU.
6. **Stability**: repeated runs must not change any value they report (a differing
   `HDREnabled`/`SDRWhiteLevel` between two runs is a write, i.e. a defect).

Not claimed by this work: vendor DisplayHDR whitelist verdict, DisplayHDR tier, GPU
(retro) scaling state, and any HDR/scale *write* path.
