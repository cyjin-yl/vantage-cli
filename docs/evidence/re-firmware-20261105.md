# UEFI/BIOS firmware identity, ESRT and last update attempt — 21VG — 2026-11-05

Target: TB14P-EZRA, ThinkBook 14 G8+ (21VG, Panther Lake), Windows 11 build
26100, firmware `SWCN22WW` dated 07/15/2026, Secure Boot on, booted UEFI.

Question this answers: recover the ESRT (EFI System Resource Table) and the
BIOS version/date/release-info sources on this SKU, determine whether Lenovo
exposes a firmware capsule update descriptor, and back a read-only
`vantage firmware status`.

**Session scope: read-only.** No capsule was staged, flashed or cleared; no
`SetupDiSetDeviceProperty`, `FirmwareResources` write, `pnputil /add-driver`,
`/delete-driver`, update installer or BCD write was executed. Every probe was a
`SELECT`-shaped WMI query, a registry read, a `Get*` API call or a
`SetupDiGet*` read.

Probe scripts and raw output on the target (all read-only, kept for replay):

```text
C:\Users\ezra\fw-probe1.ps1 -> C:\Users\ezra\fw-probe\p1.txt   (BIOS, PnP class, Enum\UEFI, namespaces)
C:\Users\ezra\fw-probe2.ps1 -> ...\p2.txt                      (device properties, 80497100 group, SMBIOS raw)
C:\Users\ezra\fw-probe3.ps1 -> ...\p3.txt                      (property store tree, FirmwareResources, full ESRT)
C:\Users\ezra\fw-probe4.ps1 -> ...\p4.txt                      (HardwareID/REV derivation, value types, SMBIOS Type 0)
```

## 1. BIOS version / date / release-info sources

Three independent projections of the same SMBIOS Type 0 record agree:

| source | live values |
| --- | --- |
| `root\CIMV2 Win32_BIOS` | `Manufacturer=LENOVO`, `SMBIOSBIOSVersion=SWCN22WW`, `ReleaseDate=2026/7/15 8:00:00`, `BIOSVersion={LENOVO - 1, SWCN22WW, INSYDE Corp. - 71250022}`, `SMBIOSMajorVersion=3`, `SMBIOSMinorVersion=8`, `SystemBiosMajorVersion=1`, `SystemBiosMinorVersion=22`, `EmbeddedControllerMajorVersion=1`, `EmbeddedControllerMinorVersion=22`, `Version="LENOVO - 1"` |
| `HKLM\HARDWARE\DESCRIPTION\System\BIOS` | `BIOSVendor=LENOVO`, `BIOSVersion=SWCN22WW`, `BIOSReleaseDate=07/15/2026`, `BiosMajorRelease=1`, `BiosMinorRelease=22`, `ECFirmwareMajorRelease=1`, `ECFirmwareMinorRelease=22`, `SystemProductName=21VG`, `SystemSKU=LENOVO_MT_21VG_BU_idea_FM_ThinkBook 14 G8+ IPH`, `BaseBoardProduct=LNVNB161216` |
| `root\WMI MSSmBios_RawSMBiosTables` | `SmbiosMajorVersion=3 SmbiosMinorVersion=8 Size=5837 Used20CallingMethod=False`; Type 0 length `26` (0x1A), strings `LENOVO \| SWCN22WW \| 07/15/2026` |

SMBIOS Type 0 offsets from the raw table: `0x14/0x15 = 1/22` (BIOS major/minor),
`0x16/0x17 = 1/22` (EC firmware major/minor), `0x18/0x19 = 32 / unit 0x00`
(extended ROM size, MB). The structure is `0x1A` bytes long, so the SMBIOS
3.1.0 fields at `0x1A..0x1D` (System BIOS major/minor, extended EC firmware
major/minor) are **not present**; `Win32_BIOS.SystemBiosMajorVersion/MinorVersion`
therefore reports the same `1.22` as the BIOS release rather than a separate
pair. That duplication is recorded, not "corrected".

`Win32_BIOS.Version` is `"LENOVO - 1"` — a vendor/ordinal string, never the
version. The version comes from `SMBIOSBIOSVersion` (string #2) and the
date from `ReleaseDate` (string #3, DMTF text). `BIOSVersion[]` additionally
carries the independent Insyde code-base build `INSYDE Corp. - 71250022`.

`GetFirmwareType` answers `FirmwareTypeUefi` (2), so the ESRT surface below is
expected to exist. (`CountSystemFirmwareTable('FIRM')` /
`GetSystemFirmwareTable('FIRM', 0)` returned 0 / `ERROR_INVALID_FUNCTION` under
this SSH session, which is why the implementation reads identity through
`Win32_BIOS` and the raw SMBIOS only through the documented
`MSSmBios_RawSMBiosTables` provider.)

## 2. The ESRT, as Windows projects it

There is no user-mode `GetEsrt` API. Windows parses the UEFI configuration table
and projects each entry as a **Firmware-class PnP device** under the `UEFI`
enumerator:

```text
UEFI\RES_{<FirmwareClass>}\0
HardwareID    UEFI\RES_{<guid>}&REV_<hex>   (revision-bearing id, first)
              UEFI\RES_{<guid>}
CompatibleID  UEFI\CC_00010001 (system firmware) | UEFI\CC_00010002 (device firmware)
              GenFirmwareResource
Class         Firmware  {f2e7dd72-6468-4e36-b6f1-6488f42c1b52}   (c_firmware.inf / vendor oem*.inf)
Parent        ACPI_HAL\UEFI\0
```

Ten entries exist on this machine (`Win32_PnPEntity WHERE PNPClass='Firmware'`,
`count=10`). Current version is the documented hardware-id derivation —
`&REV_<hex>` on the revision hardware id — exactly as the Microsoft reference
script `QueryVersionAndStatus.ps1` does it
(<https://learn.microsoft.com/en-us/windows-hardware/manufacture/desktop/query-version-and-status-ps1-script>):

> "The current firmware version for a firmware resource device is stored in its
> Hardware ID. The HW ID is of the form: UEFI\RES_{<GUID>}&REV_XXXX where XXXX
> is the current firmware version in Hex."

| # | firmware class (`ESRT FirmwareClass`) | device name | `&REV_` hex | version | driver |
| --- | --- | --- | --- | --- | --- |
| 1 | {29D8988F-BCB4-47D1-9D0C-F2C08FB1E91E} | Device Firmware | `1000009` | 16777225 | c_firmware.inf 10.0.26100.4768 (Microsoft) |
| 2 | {832AF090-2EF9-7C47-8F6D-B405C8C7F156} | Device Firmware | `82802` | 534530 | c_firmware.inf 10.0.26100.4768 |
| 3 | {E4099DA4-D91C-43F7-A3D3-DF369B1F3A1D} | Realtek PD Firmware Capsule | `1A021B` | 1704475 | oem43.inf 1.0.1.0 (Realtek) |
| 4 | {64EC2CA6-4AE3-4A6E-B542-27A7E0AA0BA6} | Device Firmware | `0` | 0 | c_firmware.inf |
| 5 | {7DA7BBF4-AF32-7E48-A65F-02C0CF62BEB0} | Device Firmware | `110` | 272 | c_firmware.inf |
| 6 | {01F69777-46DB-46D0-9947-83BAC67B7EF1} | Device Firmware | `1011` | 4113 | c_firmware.inf |
| 7 | {499371BF-8950-4B3C-81B6-2C97525241D7} | Goodix Update UEFI | `D033710` | 218314512 | oem57.inf 0.0.7.9 (Goodix) |
| 8 | {94ECDA5A-E410-411E-A7FB-5B82A0456A47} | Device Firmware | `0` | 0 | c_firmware.inf |
| 9 | {865D322C-6AC7-4734-B43E-55DB5A557D63} | Device Firmware | `564` | 1380 | c_firmware.inf |
| 10 | {8163650E-C940-3926-0703-FCF1F743D4E2} | System Firmware | `71250022` | 1898250274 | c_firmware.inf |

All ten report `Status=OK`, `ConfigManagerErrorCode=0`.

### 2.1 Independent cross-check: the firmware-resource property store

The same devices carry the ESRT fields as device properties in the
`{80497100-8C73-48B9-AAD9-CE387E19C56E}` property group (read live with
`Get-PnpDeviceProperty`):

| pid | property | type | live (System Firmware) |
| --- | --- | --- | --- |
| 2 | `DEVPKEY_FirmwareResource_Id` | GUID | `{8163650E-C940-3926-0703-FCF1F743D4E2}` |
| 3 | `DEVPKEY_FirmwareResource_Type` | UINT32 | `1` (system firmware); all other entries `2` (device firmware) |
| 4 | `DEVPKEY_FirmwareResource_Version` | UINT32 | `1898250274` = `0x71250022` |
| 5 | `DEVPKEY_FirmwareResource_LowestSupportedVersion` | UINT32 | `1380122624` = `0x52430000` |
| 6 | unnamed in this SDK | UINT32 | `0` |
| 7 | unnamed in this SDK | UINT64 | `0` |

**All ten entries agree** between the hardware-id derivation and pid 4
(1000009/16777225, 82802/534530, 1A021B/1704475, 0, 110/272, 1011/4113,
D033710/218314512, 0, 564/1380, 71250022/1898250274). The two sources are
independent projections of the same ESRT byte field, so the agreement is
meaningful; the property store is read in the implementation purely as this
cross-check, and a disagreement is reported as a per-entry `issues` string
rather than being resolved by picking one value.

Pids 6 and 7 are not named by the `windows-sys` key set this build links
against and this session did not identify them; they are **not** used by the
implementation. A different build of an SDK may name them.

`LowestSupportedVersion` is not all zero: entry 6 is `1`, entry 9 is `1000`
(`0x3E8`) and the System Firmware entry is `0x52430000`.

## 3. Last attempt status

The firmware update driver records the attempt per firmware class under:

```text
HKLM\SYSTEM\CurrentControlSet\Control\FirmwareResources\{<FirmwareClass>}\
```

(Row values and types, `GetValueKind`, live:)

```text
Version              DWord   218314512        (0x0D033710)
Filename             String  \SystemRoot\System32\DriverStore\FileRepository\
                             goodixtouchpad.inf_amd64_ca04c9cdd29c9242\
                             TB14G8+_TFforcepad_FW0D033710_260720.bin
Phase                DWord   2
Catalog              String  oem57.cat
LastAttemptVersion   DWord   218314512
LastAttemptStatus    DWord   0
```

Only **one** class has a record on this machine: `{499371BF-8950-4B3C-81B6-2C97525241D7}`
(Goodix). The other nine entries, including the System Firmware entry, have no
record — the key is written when a driver stages a capsule for that class, so an
absent record is reported as "none recorded", never as success.

Status decoding follows the ESRT field definition and nothing else:

| value | meaning | printed |
| --- | --- | --- |
| `0` | last attempt completed successfully | `0 (success)` |
| `0xFFFFFFFF` | no attempt was made | `4294967295 (no attempt)` |
| anything else | vendor/OEM-defined, reserved by the specification | `<raw> (unnamed)` — never `success` |

`Phase` is reported verbatim and is not interpreted: this session found no
documented enumeration for it, so naming it would be a guess.

## 4. Does Lenovo expose a firmware capsule update descriptor?

**No vendor capsule descriptor from Lenovo exists on this machine.** The
driver-store census (`pnputil /enum-drivers`, `Class Name: Firmware` /
`{f2e7dd72-6468-4e36-b6f1-6488f42c1b52}`) contains exactly two firmware-class
INFs, both from component vendors:

| INF | provider | version | binds |
| --- | --- | --- | --- |
| `oem57.inf` (`goodixtouchpad.inf`) | Goodix | 0.0.7.9 | `UEFI\RES_{499371BF-…}` (touchpad controller firmware) |
| `oem43.inf` (`rtkfwcapsule.inf`) | Realtek Semiconductor Corp. | 1.0.1.0 | `UEFI\RES_{E4099DA4-…}` (PD controller firmware) |

The remaining eight entries are bound by Microsoft's `c_firmware.inf`
(`FirmwareResource.NT`, `UEFI\CC_00010001`/`UEFI\CC_00010002`). That is the
generic ESRT device driver Windows installs for the firmware's own table — it is
not a Lenovo capsule package, and no Lenovo firmware-update INF is present.

This matches the project charter: the Lenovo BIOS/EC update path is a UDC
package installed by Lenovo software (`docs/11-sysupdate-diagnostics.md` §1/§8
list IA/BootInstaller firmware flashing as **excluded**). `firmware status`
therefore reports the firmware's own ESRT capability and the driver's attempt
record; it adds **no** install or flash verb.

## 5. What was implemented (read-only)

| file | change |
| --- | --- |
| `crates/lctrl-core/src/firmware.rs` | new: `FirmwareType`, `FirmwareIdentity`, `FirmwareResource`, `FirmwareAttempt`, `FirmwareStatus`, `firmware_class`, `firmware_revision`, `format_firmware_version`, `last_attempt_status_meaning`, plus unit tests for the parsers and the status naming |
| `crates/lctrl-core/src/lib.rs` | `mod firmware;` + re-exports |
| `crates/lctrl-core/src/command_capability.rs` | `CommandId::FIRMWARE_STATUS = "firmware.status"`, added to `CommandId::ALL` |
| `crates/lctrl-hal/src/firmware.rs` | new: `trait FirmwareControl { fn firmware_status(&self) -> Result<FirmwareStatus>; }` — one method, a read |
| `crates/lctrl-hal/src/lib.rs` | module + `pub use firmware::FirmwareControl;` |
| `crates/lctrl-hal/src/command_capability.rs` | row: `read_only(StateLayer::BiosPersistent)`, `FeatureBinding::Capability("firmware.status")`, `ServiceBinding::One(ServiceRole::Hal)`, deps `required(PublicWin32Cim)`, `optional(Win32SetupApi)`, `optional(Win32Registry)`, `DispatchSupport::Implemented` |
| `crates/lctrl-hal-win/src/firmware.rs` | new: `WindowsFirmware<W: WmiTransport>` + `probe_firmware_status`; WMI reads in the cross-platform part, SetupAPI/registry reads in `#[cfg(windows)] mod native` |
| `crates/lctrl-hal-win/src/firmware_tests.rs` | new: fake-transport tests for identity/resource mapping, absent-version serialization, the disagreement path, and error propagation |
| `crates/lctrl-hal-win/src/lib.rs` | module + `pub use firmware::WindowsFirmware;` |
| `crates/lctrl-hal-win/src/windows_hal.rs` | records `firmware.status` from the live firmware-class device probe |
| `crates/vantage-cli/src/lib.rs` | `Command::Firmware { command }` + `FirmwareCommand::Status`, `CommandServices.firmware`/`with_firmware`, dispatch, `feature()` mapping, `execute_firmware_status` |
| `crates/vantage-cli/src/capability.rs` | `Command::Firmware` → `StateLayer::BiosPersistent` and `CommandId::FIRMWARE_STATUS` |
| `crates/vantage-cli/tests/cli_contract.rs` | three contract tests over a fake firmware service |
| `vantage/src/main.rs` | Windows composition root attaches `WindowsFirmware::new(NativeWmi)`; the Linux root attaches nothing, so `firmware status` reports `Unsupported` |

Channel mapping of the rows the command prints:

| row | source |
| --- | --- |
| vendor / version / release date / BIOS release / EC / SMBIOS / version strings | `Win32_BIOS` (`ROOT\CIMV2`) |
| firmware type | `GetFirmwareType` |
| ESRT entry list, description, status | `Win32_PnPEntity WHERE PNPClass='Firmware'` |
| current version | `HardwareID` `&REV_<hex>` (documented derivation) |
| property-store version (cross-check) | `SetupDiGetDevicePropertyW` `{80497100-…}` pid 4 on the `UEFI` enumerator |
| attempt version/status/phase/image/catalog | `HKLM\SYSTEM\CurrentControlSet\Control\FirmwareResources\{guid}` |

Human output on this machine (values from the probes above; the binary itself is
the parent's to build and run):

```text
UEFI firmware: LENOVO SWCN22WW, dated 2026/7/15 8:00:00
  BIOS release: 1.22
  EC firmware: 1.22
  SMBIOS: 3.8
  version strings: LENOVO - 1, SWCN22WW, INSYDE Corp. - 71250022
ESRT entries: 10
  {8163650E-C940-3926-0703-FCF1F743D4E2}  System Firmware  version 1898250274 (0x71250022)  [OK]
    property store: 1898250274 (0x71250022)
  ...
update attempts: 1
  {499371BF-8950-4B3C-81B6-2C97525241D7}  version 218314512 (0xD033710), last attempt 218314512 (0xD033710), status 0 (success)
    phase: 2
    image: \SystemRoot\...\TB14G8+_TFforcepad_FW0D033710_260720.bin
    catalog: oem57.cat
```

JSON is the `FirmwareStatus` serialization: `identity`, `resources[]` (with
`current_version` and `stored_version` **always present**, `null` when absent),
`attempts[]`, and `issues[]` when a read was incomplete.

## 6. Limits and things this session did not establish

- **No binary was built or run** in this seat (source-only). The human output
  above is composed from the probe values, not from a `vantage` run; the parent
  owns build + live acceptance.
- The `Phase` enumeration in `FirmwareResources` is unknown and stays raw.
- Pids 6/7 of the firmware-resource property group are unidentified and unused.
- `GetSystemFirmwareTable('FIRM', …)` failed with `ERROR_INVALID_FUNCTION` in
  this SSH session; the raw SMBIOS was read through
  `root\WMI MSSmBios_RawSMBiosTables` instead. The CLI does not depend on either.
- The ESRT/attempt tables are a snapshot of this boot. A firmware update would
  change the revisions and add an attempt record; nothing here caches them.
- The firmware-class device list is read through WMI; if `Win32_PnPEntity`
  fails, the command fails with that error rather than printing an empty table.

## 7. Acceptance the parent should run

1. `cargo test -p lctrl-core firmware` — parser, status-naming and
   `reported_version` unit tests (run on any platform).
2. `cargo test -p lctrl-hal-win firmware` — the fake-transport mapping tests,
   including the mismatch (`hardware-id revision` vs property store) and the
   absent-version serialization assertions.
3. `cargo test -p vantage-cli` — the three new contract tests: identity/ESRT/
   attempt rendering, an **unknown** `LastAttemptStatus` printed as
   `4096 (unnamed)` with a `null` outcome in JSON, and `Unsupported` when no
   firmware service is attached.
4. On tb14p-ezra: `vantage firmware status` and
   `vantage firmware status --json`; compare every `current_version` against
   `Get-PnpDeviceProperty -InstanceId <id> -KeyName DEVPKEY_FirmwareResource_Version`
   (the ten values must match §2/§2.1).
5. `vantage capability` — `firmware.status` should appear as `available`
   (`FeatureBinding::Capability("firmware.status")` now has a recorder in
   `windows_hal::capabilities`); on a legacy-BIOS machine or when the ESRT query
   returns nothing it must read `unavailable`, not `unverified`.
6. Negative controls: `vantage firmware --help` must list only `status`; the
   crate must not reference `SetupDiSetDeviceProperty`, `FirmwareResources`
   writes, `pnputil /add-driver`, or any update installer; and
   `vantage firmware status` on a machine whose ESRT is empty must print
   `ESRT entries: 0` plus the UEFI-no-entry issue rather than a synthetic row.
