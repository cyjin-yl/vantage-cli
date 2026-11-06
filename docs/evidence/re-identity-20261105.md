# Device identity + firmware inventory — authoritative sources on 21VG

Slice: `IDENTITY` (device identity / firmware rollup for `vantage info`).
Target: `tb14p-ezra` = ThinkBook 14 G8+ IPH (`21VG`), Windows 11 build 26300.
Collected **read-only** on 2026-11-05/06 over SSH: CIM queries, registry reads,
`GetSystemFirmwareTable('RSMB')`, and static analysis of Lenovo assemblies copied
off the target. Nothing was written, installed, started, or reconfigured on the
target; no vendor binary was executed.

Question: for each identity/firmware field that `vantage info` reports (or should
report), which source on this SKU is authoritative, and which plausible-looking
source is **not**?

---

## 1. Field → source table (live values)

`root\CIMV2` unless stated. "Corroborated by" lists independently queried
surfaces that returned the same value.

| Field in `info` | Authoritative source | Live value | Corroborated by |
|---|---|---|---|
| machine type model (MTM) | `Win32_ComputerSystem.Model` (mirrors `Win32_ComputerSystemProduct.Name`) | `21VG` | SMBIOS Type 1 Product Name = `21VG`; Lenovo `HKLM\SOFTWARE\WOW6432Node\Lenovo\MachineInfo::MTM` = `21VG` |
| marketing product version | `Win32_ComputerSystemProduct.Version` | `ThinkBook 14 G8+ IPH` | SMBIOS Type 1 Version; `HKLM\HARDWARE\DESCRIPTION\System\BIOS::SystemVersion`; `…\Lenovo\MachineInfo::Version` |
| family | `Win32_ComputerSystem.SystemFamily` | `ThinkBook 14 G8+ IPH` | SMBIOS Type 1 Family; `…\System\BIOS::SystemFamily` |
| system SKU (**not emitted**: `ProductInventory` has no SKU field) | `Win32_ComputerSystem.SystemSKUNumber` | `LENOVO_MT_21VG_BU_idea_FM_ThinkBook 14 G8+ IPH` | SMBIOS Type 1 SKU Number; `…\System\BIOS::SystemSKU` |
| serial number | `Win32_ComputerSystemProduct.IdentifyingNumber` | present, 8 chars, `YX0J****` | `Win32_BIOS.SerialNumber`, `Win32_BaseBoard.SerialNumber`, SMBIOS Type 1 Serial, `…\Lenovo\MachineInfo::SerialNumber` — all identical |
| product UUID | `Win32_ComputerSystemProduct.UUID` | present, non-zero, non-all-FF (masked) | SMBIOS Type 1 UUID bytes |
| baseboard product | `Win32_BaseBoard.Product` | `LNVNB161216` | SMBIOS Type 2 Product; `…\System\BIOS::BaseBoardProduct` |
| baseboard version | `Win32_BaseBoard.Version` | `SDK0T76577 WIN` | SMBIOS Type 2 Version; `…\System\BIOS::BaseBoardVersion` |
| BIOS version | `Win32_BIOS.SMBIOSBIOSVersion` | `SWCN22WW` | SMBIOS Type 0 BIOS Version string; `…\System\BIOS::BIOSVersion`; `Win32_BIOS.Name` |
| BIOS release date | `Win32_BIOS.ReleaseDate` (DMTF datetime text, reported verbatim) | `20260715000000.000000+000` | SMBIOS Type 0 Release Date string `07/15/2026`; `…\System\BIOS::BIOSReleaseDate` = `07/15/2026` |
| SMBIOS version | `Win32_BIOS.SMBIOSMajorVersion`/`.SMBIOSMinorVersion` | `3.8` | `RawSMBIOSData` header bytes 1–2 = `03 08` |
| EC firmware version | `Win32_BIOS.EmbeddedControllerMajorVersion`/`.EmbeddedControllerMinorVersion` | `1` / `22` → `1.22` | SMBIOS Type 0 offsets 0x16/0x17 = `01 16`; `…\System\BIOS::ECFirmwareMajorRelease`/`ECFirmwareMinorRelease` = 1/22 |

Every `…\System\BIOS` citation is `HKLM\HARDWARE\DESCRIPTION\System\BIOS`.

### Values that must **not** be used as the firmware version

| Surface | Observed | Why it is not the version |
|---|---|---|
| `Win32_BIOS.Version` | `LENOVO - 1` | firmware vendor + ordinal string (kept in the record, never displayed as the version) |
| `Win32_BIOS.BIOSVersion[0]` | `LENOVO - 1` | same field family, same reason; `BIOSVersion[1]` is `SWCN22WW`, `[2]` is `INSYDE Corp. - 71250022` |
| `Win32_BIOS.Description`/`.Caption` | `SWCN22WW` | derived label, not an independent read |
| **Lenovo cached `…\SOFTWARE\WOW6432Node\Lenovo\MachineInfo::BIOS Version`** | **`SWCN15WW`** (stale) | written once at factory OOBE (`AuditDate` = `2026-01-29T03:29:57`, `CSUP` = `12-13-2025`); live firmware is `SWCN22WW`. A registry-first implementation would report a BIOS version two revisions old |

---

## 2. EC firmware version — recovered provenance

The EC/BIOS firmware revisions are the SMBIOS Type 0 (BIOS Information)
extension fields. Raw `RawSMBIOSData` from `GetSystemFirmwareTable('RSMB')`
(provider `0x52534D42`), table length 5837, SMBIOS 3.8:

```
type=0 handle=0 len=26
hex=001a00000102000003ff8098f94b00000a01030d011601162000
        ^^ len=0x1a                                ^^^^^^ ^^ ^^
                                                     0x14 0x15 0x18   (0x18 = extended ROM size 0x20)
```

Offsets used (SMBIOS 3.1+ Type 0 layout):

| Offset | Meaning | Byte | Value |
|---|---|---|---|
| 0x14 | System BIOS major release | `01` | 1 |
| 0x15 | System BIOS minor release | `16` | 22 |
| 0x16 | EC firmware major release | `01` | 1 |
| 0x17 | EC firmware minor release | `16` | 22 |

Windows surfaces exactly these bytes as `Win32_BIOS.EmbeddedControllerMajorVersion`
(`CimType = UInt8`, value 1) / `…MinorVersion` (UInt8, 22), and mirrors them into
`HKLM\HARDWARE\DESCRIPTION\System\BIOS` as `ECFirmwareMajorRelease` /
`ECFirmwareMinorRelease`. `Win32_BIOS.SystemBiosMajorVersion`/`.SystemBiosMinorVersion`
= 1/22 comes from the adjacent 0x14/0x15 pair and is *not* the EC value.

### What Lenovo's own binaries read

Two Lenovo components were copied off the target and string-analysed (read-only,
no execution). Both name the same CIM properties — no registry or IOCTL EC path
exists in either:

| Assembly (path, file version, sha256) | Proof strings |
|---|---|
| `…\ProgramData\Lenovo\Vantage\Addins\LenovoSystemUpdateAddin\1.0.36.6\Client.dll` — 1.0.36.6, `f9b1c9c78c78dcddbf060f094a06a80a7b197dc9b83d203aba4b8188d552e21f` | method `GetECVersionByWMI`; `get_EcVersion`; property literals `EmbeddedControllerMajorVersion`, `EmbeddedControllerMinorVersion`; `SELECT * FROM Win32_Bios`; siblings `GetBIOSVersionByWMI` (`SMBIOSBIOSVersion`, `SystemBiosVersion`), `GetBIOSReleaseDateByWMI` |
| `…\LenovoSnapshotAddin\2.2.26070.2\Lenovo.Cdat.dll` — 2.2.26070.2, `87c66718de51924a81b91cf87606b74e4862a49a1e95027dadb8f3aade9a283a` | `get_EcVersion`/`set_EcVersion`; `EmbeddedControllerMajorVersion`, `EmbeddedControllerMinorVersion`; `Win32_BIOS`; snapshot model `CaptureSMBIOSVersion` |

Supporting assemblies (same conclusion, same SMBIOS origin):

| Assembly | Proof strings |
|---|---|
| `…\VantageService\5.1.2609.9\Lenovo.VantageService.Utilities.dll` (4.0.7.2, `252ececa29…29aa27`) | `GetSystemFirmwareTable`, `RSMB`, `Lenovo.VantageService.Utilities.DeviceInformation.Smbios.SmbiosV2`, `SmbiosStringParser`, `rawSmbiosTable`, `<EcVersion>`, `<BiosVersion>`, `<Mtm>`, `LenovoSmbios`, `SmbiosType200Property` — Vantage's own SMBIOS reader; **no SMBIOS Type 200 (0xC8, Lenovo OEM) structure exists in this machine's table** (types present: 0,1,2,3,4,7,8,9,11,12,13,14,15), so the Lenovo-OEM path yields nothing here and the standard Type 0/1/2 fields are the only firmware identity |
| `…\VantageCoreAddin\1.1.0.67\VantageCoreAddin.dll` (1.1.0.67, `d71375189f…b6899f`) | `<EcVersion>`, `<BiosVersion>`, `<Mtm>`, `<SerialNumber>`, `GetMachineInfo`, `MachineInfoCommand` |
| `…\GenericMessagingAddin\1.1.0.209\GenericMessagingAddin.Shared.dll` (1.1.0.209, `fc65d1e25a…9d71`) | `<ECVersion>k__BackingField`, `get_ECVersion`, `GetMachineInfo`, `IMachineInformationAgent` |

**Important observed ambiguity (not resolved, deliberately not "corrected"):**
on this unit the EC revision (1.22) is numerically identical to the BIOS
revision (1.22). The two are distinct Type 0 fields (0x16/0x17 vs 0x14/0x15) and
the firmware asserts both, but no independent EC image identifier (e.g. an
`SWEC…` string) exists anywhere on the machine to distinguish "EC 1.22" from a
BIOS-revision mirror. The CLI therefore reports the Type 0 EC revision as
delivered, sourced to that field, and does not claim an independent EC image
version.

[INFERENCE] `GetECVersionByWMI` binds `SELECT * FROM Win32_Bios` to the two
`EmbeddedController*Version` property literals in the same assembly; the method
body was not disassembled. The *value* provenance does not depend on that
inference: the bytes are in SMBIOS Type 0 and the CIM/registry copies match them
byte-for-byte.

---

## 3. Identity surfaces that carry nothing useful (checked, excluded)

| Surface | Observed |
|---|---|
| `root\WMI` `Lenovo_*` classes | event/diagnostic classes only (`LENOVO_DISPATCHER_EVENT`, `LENOVO_GAMEZONE_*`, `LENOVO_REPORT_*`, `LENOVO_BTKBD_*`, …); no identity/version class, no `root\Lenovo` namespace |
| `root\WMI` `Lenovo_BiosSetting` | 53 setting rows, all `Name,Value` setup options; none carries BIOS/EC/MTM identity |
| Lenovo UDC applicability tags | `DaysSinceOobe`, `OobeDate`, NPU/warranty/expiry tags, Speech app tags — no firmware-version applicability for this device |
| `HKLM\SOFTWARE\Lenovo\UDC` package tree | install metadata and the package security catalogs; no EC/firmware version |
| `Win32_PnPSignedDriver` `DeviceClass='FIRMWARE'` | 10 rows, all Microsoft `UEFI\RES_*` capsules plus `Realtek PD Firmware Capsule` and `Goodix Update UEFI` — these are *update driver package* versions, not the device firmware revision (the record type already documents this) |

Adjacent observation for routing (not part of this slice): live
`Lenovo_BiosSetting` enumeration returns **53** rows, while
`docs/05-bios-settings.md` records 36 (`ISET_0`–`ISET_52`, 23 non-empty).

---

## 4. What `vantage info` reports now

Existing keys/lines are unchanged (`platform`, `hardware.product_name`,
`hardware.family`, `hardware.bios_version`, `features`, and the four human lines
`platform:`/`product:`/`family:`/`bios:`). The identity rollup is additive:

JSON (`identity` key, omitted when the HAL reports no identity module at all):

```json
"identity": {
  "product":     { "name", "vendor", "version", "identifying_number", "uuid",
                   "manufacturer", "model", "family" },
  "motherboard": { "manufacturer", "product", "serial_number", "version" },
  "bios":        { "manufacturer", "name", "version", "smbios_bios_version",
                   "version_strings", "release_date", "serial_number", "status",
                   "smbios_major_version", "smbios_minor_version",
                   "embedded_controller_major_version",
                   "embedded_controller_minor_version" }
}
```

These are the existing `lctrl-core` inventory records, not a second schema:
`ProductInventory` / `MotherboardInventory` / `BiosInventory` from
`crates/lctrl-core/src/inventory.rs`.

Human output on this target (values from §1):

```
platform: windows
product: 21VG
family: ThinkBook 14 G8+ IPH
bios: SWCN22WW
identity:
  model (mtm): 21VG
  product version: ThinkBook 14 G8+ IPH
  serial: YX0J****
  baseboard: LNVNB161216
  baseboard version: SDK0T76577 WIN
  bios release date: 20260715000000.000000+000
  ec firmware: 1.22
features:
  …
```

* `serial`/`uuid` are emitted to the local caller exactly as the provider gives
  them (Vantage's own UI shows both); they are masked in this document only.
* The existing `product:` line is unchanged and still reports
  `Win32_ComputerSystemProduct.Name`; on this SKU that value **is** the MTM
  (`21VG`), which is why the identity block adds a separate
  `product version:` line for the marketing name (`ThinkBook 14 G8+ IPH`).
  `docs/08-architecture.md` §5 already specifies the `product:` line as the
  SMBIOS Product Name, so the line keeps its documented meaning.
* `bios release date` is the `Win32_BIOS.ReleaseDate` DMTF text **verbatim**. No
  reformatting: `(Get-WmiObject Win32_BIOS).ReleaseDate` is a `System.String`
  with value `20260715000000.000000+000` in the same channel the Rust transport
  reads, so `WmiValue::String` carries it through unchanged.
* `ec firmware` is rendered only when **both** EC fields are present
  (`BiosInventory::embedded_controller_version`); a half-read revision is absent,
  never guessed.
* A field the provider omits prints `unknown` rather than borrowing another
  class or the registry.

### Implementation (bounded, read-only)

| Change | File |
|---|---|
| `SystemIdentity { product, motherboard, bios }` + `is_empty()`, `BiosInventory::embedded_controller_version()` | `crates/lctrl-core/src/inventory.rs`, exported in `crates/lctrl-core/src/lib.rs` |
| `Hal::system_identity()` default (all-absent) so existing impls/fakes keep compiling | `crates/lctrl-hal/src/capability.rs` |
| `WindowsInventory::capture_identity()` / `identity_from_wmi()` — the 4 identity queries only (`Win32_ComputerSystemProduct`, `Win32_ComputerSystem`, `Win32_BaseBoard`, `Win32_BIOS`), provenance table in the doc comment | `crates/lctrl-hal-win/src/system_inventory.rs` |
| `WindowsHal::system_identity()` delegates to that bounded reader | `crates/lctrl-hal-win/src/windows_hal.rs` |
| `identity` payload key + identity lines in `format_info` | `crates/vantage-cli/src/lib.rs` |

Tests added (not run in this slice — the parent owns verification):
`crates/lctrl-hal-win/tests/windows_system_inventory.rs`
(`identity_capture_reads_only_product_baseboard_and_bios` uses a transport that
**fails** on any class outside the four, so a widened query set is an error;
`identity_capture_without_provider_records_stays_empty`;
`embedded_controller_version_requires_both_provider_fields`) and
`crates/vantage-cli/tests/cli_contract.rs`
(`info_reports_identity_modules_next_to_unchanged_hardware_keys`,
`info_human_output_lists_identity_values_and_omits_an_absent_identity`).

---

## 5. Not verified here

* Nothing was built, linked, or executed in this slice: no `cargo check`, no
  Windows binary, no native `vantage info` run. The Rust-side readings are code
  paths over the same WMI classes proven above, not captured program output.
* Linux `Hal::system_identity` is the default (all-absent); `/sys/class/dmi/id`
  mapping for `info` identity is **not** implemented in this slice.
* `Win32_ComputerSystem.SystemSKUNumber` is read by no field of
  `ProductInventory`, so the SKU appears in this document only; adding it would
  extend the shared inventory record and was left out of the bounded change.
* The 21VG EC equals BIOS revision on this unit; no second machine or older
  BIOS was sampled to see whether they separate.

## 6. Suggested parent checks

1. `cargo check -p lctrl-hal-win --target x86_64-pc-windows-msvc` (cross) — the
   `SystemIdentity` wiring, the default trait method, and the new tests.
2. Windows `cargo test --offline -p lctrl-hal-win --test windows_system_inventory`
   and `-p vantage-cli --test cli_contract`.
3. Native `vantage info` / `vantage info --json` on `tb14p-ezra`, compared field
   by field with §1 (expect `ec firmware: 1.22`, `bios release date:
   20260715000000.000000+000`, `serial` matching the 8-char value across all
   sources, `hardware.*` unchanged).
4. Negative controls: rename the `MachineInfo` registry key in a scratch probe
   and confirm `info` is unaffected (it reads no registry); confirm a machine
   without `Win32_BaseBoard`/EC fields still emits `unknown` rather than a
   borrowed value; confirm `info` performs exactly 4 WMI class queries for
   identity (bounded capture, no CPU/memory/GPU/disk/network/firmware sweep).
