# Secure Boot / TPM / BitLocker read-only contract (`security status`) — recon — 2026-11-05

Scope: the security postures this platform already exposes, and the exact read-only sources
`security status` may use. No writes, no key material, no TPM ownership/attestation operations.

Machine: `tb14p-ezra` (ThinkBook 21VG), Windows 11 build 26300, UEFI, elevated SSH token
(`tb14p-ezra\ezra`, Administrators, `SeSystemEnvironmentPrivilege` **Enabled**).
Every receipt below was produced by a read-only `Get-*` query or a read-only Win32 read during this
recon; no `Set*`, no `Invoke-CimMethod`, no BitLocker/TPM operation was issued.

## 1. Verdict

| item | authoritative read-only source | state on this machine |
|---|---|---|
| Secure Boot | UEFI variable `SecureBoot` in `{8be4df61-93ca-11d2-aa0d-00e098032b8c}` read with `GetFirmwareEnvironmentVariableExW` | **enabled** (`len=1`, byte `0x01`; attributes `0x00000006` = BS+RT) |
| TPM | `root\CIMV2\Security\MicrosoftTpm: Win32_Tpm` (singleton, property-only query) | **present**, 2.0 (Intel PTT: `INTC`, `700.21.2.1380`), enabled/activated/owned |
| BitLocker | `root\CIMV2\Security\MicrosoftVolumeEncryption: Win32_EncryptableVolume` (property-only query) | **C: fully decrypted, protection off** |

Two independent sources agree on Secure Boot, and none of them is a vendor component:

1. UEFI `SecureBoot` variable = `1`.
2. `HKLM\SYSTEM\CurrentControlSet\Control\SecureBoot\State!UEFISecureBootEnabled` = `1` (REG_DWORD),
   sibling values `PolicyPublisher={77fa9abd-0359-4d32-bd60-28f4e78f784b}` (Microsoft Windows
   Production PCA), `PolicyVersion=1`.
3. `Confirm-SecureBootUEFI` (the cmdlet the OS ships) returned `True`. This is the *same* variable
   read through the SecureBoot module; the implementation does **not** spawn PowerShell, it reads
   the variable directly (see §5).
4. The Lenovo ACPI BIOS mapper (`root\WMI: Lenovo_BiosSetting`) reports
   `SecureBoot,Enable` — the same setting the frozen `bios get SecureBoot` path already exposes,
   which is a firmware-setting view, not the runtime Secure Boot state.

## 2. Secure Boot — the exact read

- API: `kernel32!GetFirmwareEnvironmentVariableExW(name, guid, buffer, size, &attributes)`.
  It reads the boot-services variable set; it cannot write (`SetFirmwareEnvironmentVariableExW`
  is a different entry point and is not imported anywhere in this tree).
- Variable: `SecureBoot`, GUID `{8be4df61-93ca-11d2-aa0d-00e098032b8c}` (UEFI global variable
  namespace). UEFI spec semantics: 1 byte, `0x00` = disabled, `0x01` = enabled.
- Corroborating variables read the same way:
  - `SetupMode` = `len=1 byte=0x00` → platform is in *user* mode (keys enrolled, Secure Boot
    enforcing). `1` would mean setup mode.
  - `PK` = `len=852 attr=0x00000027` → Platform Key present (attributes include
    `EFI_VARIABLE_NON_VOLATILE|BOOTSERVICE_ACCESS|RUNTIME_ACCESS|TIME_BASED_AUTHENTICATED_WRITE`).
    Only presence and length are reported; the bytes are discarded and never logged.
  - `KEK` = `len=3066 attr=0x00000027` (presence not reported by `security status`; recorded here
    only as evidence the namespace is readable).
- Firmware mode: `kernel32!GetFirmwareType` → `ok=True type=2` (`FirmwareTypeUefi`). Under
  `FirmwareTypeBios` (1) Secure Boot does not exist and the command reports it as
  not-applicable rather than "disabled".
- **Privilege**: the read requires `SeSystemEnvironmentPrivilege` in the calling token. On this
  elevated token it is already `Enabled`:
  `SeSystemEnvironmentPrivilege  Modify firmware environment values  Enabled`.
  The implementation enables it explicitly (`OpenProcessToken` +
  `LookupPrivilegeValueW(SE_SYSTEM_ENVIRONMENT_NAME)` + `AdjustTokenPrivileges`) and treats
  `ERROR_ACCESS_DENIED`/`ERROR_PRIVILEGE_NOT_HELD` as *unavailable with reason*, never as
  "disabled".

Correction recorded so it is not repeated: `db`/`dbx`/`dbt` live in the **image security database**
GUID `{d719b2cb-3d3a-4596-a3bc-dad00e67656f}`, not in the global GUID. Reading `db` in the global
GUID returns `ERROR_ENVVAR_NOT_FOUND (203)`; that is not evidence of a missing signature database.
`security status` does not read the image database at all.

## 3. TPM — `Win32_Tpm` properties

Namespace/class: `root\CIMV2\Security\MicrosoftTpm` : `Win32_Tpm`
(MOF: `C:\Windows\System32\wbem\Win32_Tpm.mof`, 24622 bytes; provider CLSID
`{E2E5A310-ECED-444F-81D7-ACCA6AC8A1A8}`, singleton, `provider("Win32_TpmProvider")`,
`SupportsPut = "FALSE"`).

Class metadata read live (`Get-CimClass`): the class declares exactly ten properties, all
`Implemented=True`, and a method table. The property-only query used here is:

```
SELECT IsEnabled_InitialValue, IsActivated_InitialValue, IsOwned_InitialValue,
       ManufacturerId, ManufacturerIdTxt, ManufacturerVersion, ManufacturerVersionFull20,
       ManufacturerVersionInfo, PhysicalPresenceVersionInfo, SpecVersion
FROM Win32_Tpm
```

Live values:

| property | value |
|---|---|
| `SpecVersion` | `2.0, 0, 1.59` |
| `ManufacturerId` | `1229870147` (`0x494E5443` = `INTC`) |
| `ManufacturerIdTxt` | `INTC` |
| `ManufacturerVersion` | `700.21.2.1380` |
| `ManufacturerVersionFull20` | `700.21.2.1380` |
| `ManufacturerVersionInfo` | `PTL` |
| `PhysicalPresenceVersionInfo` | `1.3` |
| `IsEnabled_InitialValue` | `True` |
| `IsActivated_InitialValue` | `True` |
| `IsOwned_InitialValue` | `True` |

The Lenovo BIOS mapper independently reports `IntelPlatformTrustTechnology,Enable`, i.e. the
firmware-level PTT switch is on; that is a BIOS setting, not the OS TPM state.

**Deliberately not used.** `Win32_Tpm` also exposes `IsReady`, `IsLockedOut`,
`IsAutoProvisioningEnabled`, `GetOwnerAuth`, `Provision`, `Clear`, `TakeOwnership`, … — all
*methods*. `Get-Tpm` (the `TrustedPlatformModule` module) calls those getters and adds
`TpmReady/TpmLockedOut/AutoProvisioning/ManagedAuthLevel` (live: `Ready=True`, `LockedOut=False`,
`AutoProvisioning=Enabled`, `ManagedAuthLevel=Full`), but `security status` invokes **no provider
method**: it reports what the three `*_InitialValue` properties and the identity strings prove and
leaves the method-only facts unread. This keeps the verb a pure property read and keeps every
ownership/clear operation out of reach.

`Absent` vs `Unavailable`: a successful query with zero instances means no TPM instance
(firmware PTT/TPM off or no TPM). On a machine without the provider the namespace itself is
missing and the query fails with `WBEM_E_INVALID_NAMESPACE`; that is reported as *unavailable with
the HRESULT detail*, never as "absent".

## 4. BitLocker — `Win32_EncryptableVolume` properties + read-only getters

Namespace/class: `root\CIMV2\Security\MicrosoftVolumeEncryption` : `Win32_EncryptableVolume`
(MOF: `C:\Windows\System32\wbem\win32_encryptablevolume.mof`, 21677 bytes, provider
`Win32_EncryptableVolumeProvider`). Query used:

```
SELECT DeviceID, DriveLetter, ProtectionStatus, ConversionStatus, EncryptionMethod,
       IsVolumeInitializedForProtection, PersistentVolumeID, VolumeType
FROM Win32_EncryptableVolume
```

Live row (the only row on this machine):

| property | value |
|---|---|
| `DeviceID` | `\\?\Volume{93aba760-c50e-44b4-824f-3543bb1e5ab9}\` |
| `DriveLetter` | `C:` |
| `ProtectionStatus` | `0` |
| `ConversionStatus` | `0` |
| `EncryptionMethod` | `0` |
| `VolumeType` | `0` |
| `IsVolumeInitializedForProtection` | `False` |
| `PersistentVolumeID` | (empty) |

A property-limited `SELECT` leaves every unselected property unset (observed: only `DeviceID`,
`DriveLetter` carry values); the implementation therefore selects exactly the properties it
reports and keeps an unselected/absent one as `None` instead of an empty string.

### Instance path and the four read-only getters

`LockStatus`, `EncryptionPercentage` and the declared value maps live on *methods*, not on
properties. Four parameterless-out getters answer on this machine, each with `ReturnValue=0`, and
none of them changes state (`GetLockStatus`, `GetConversionStatus`, `GetProtectionStatus`,
`GetEncryptionMethod`). Live receipts (all four were probed; the implementation invokes the first
two, because the protection status and the encryption method are already class properties while the
lock state and conversion percentage are only getter results):

```
GetProtectionStatus   rc=0 :: ProtectionStatus=0
GetEncryptionMethod   rc=0 :: EncryptionMethod=0; EncryptionMethodFlags=0; SelfEncryptionDriveEncryptionMethod=<empty>
GetConversionStatus   rc=0 :: ConversionStatus=0; EncryptionPercentage=0; EncryptionFlags=0;
                             WipingPercentage=0; WipingStatus=4294967295
GetLockStatus         rc=0 :: LockStatus=0
GetConversionStatus   rc=0 :: (PrecisionFactor=0 passed explicitly) pct=0 cs=0
```

The instance path the provider itself reports
(`System.Management.ManagementObject.Path`, i.e. the exact grammar the Rust transport passes to
`ExecMethod`) is:

```
\\TB14P-EZRA\root\CIMV2\Security\MicrosoftVolumeEncryption:Win32_EncryptableVolume.DeviceID="\\\\?\\Volume{93aba760-c50e-44b4-824f-3543bb1e5ab9}\\"
```

`DeviceID` is the declared key property (`key=True` in the class metadata), so the provider-relative
path is `Win32_EncryptableVolume.DeviceID="<DeviceID with \\ and \" escaped>"`; `__Path` is only a
convenience when the provider returns it. `GetConversionStatus` additionally declares one IN
parameter (`PrecisionFactor`, MOF line 126) which is passed explicitly as `0` — the same value the
OS cmdlet used by default — so the call has no unset in-parameter. Each getter failure leaves that
field `None` with the reason recorded; nothing defaults to `0`/`Unlocked`.

Independent OS view (`Get-BitLockerVolume`, the `BitLocker` module over the same provider):
`C: ProtectionStatus=Off ConversionStatus=FullyDecrypted EncryptionMethod=None
EncryptionPercentage=0 LockStatus=Unlocked VolumeStatus=FullyDecrypted`, no key protectors.

### Value maps and their provenance

`VolumeType` carries the map in the class property itself:

```
ValueMap{"0","1","2"}, Values{"OSVolume","FixedDataVolume","PortableDataVolume"}
```

The other three properties carry an empty `Description` and **no `ValueMap` qualifier** in the
property declaration. The maps below are taken from the getter signatures of the *same*
observable in the same MOF (verbatim `^` lines):

```
uint32 GetConversionStatus ([OUT, ValueMap{"0","1","2","3","4","5"},
    Values{"FullyDecrypted","FullyEncrypted","EncryptionInProgress",
            "DecryptionInProgress","EncryptionPaused","DecryptionPaused"}] uint32 ConversionStatus, ...)
uint32 GetProtectionStatus([OUT, ValueMap{"0","1","2"},
    Values{"Unprotected","Protected","Unknown"}] uint32 ProtectionStatus);
uint32 GetEncryptionMethod([OUT, ValueMap{"0","1","2","3","4","5","6","7"},
    Values{"None","AES_128_WITH_DIFFUSER","AES_256_WITH_DIFFUSER","AES_128","AES_256",
            "HARDWARE_ENCRYPTION","XTS_AES_128","XTS_AES_256"}] uint32 EncryptionMethod, ...)
```

The typed fields for `ProtectionStatus`, `ConversionStatus` and `EncryptionMethod` are taken from
these declared getter maps, not from a guess, and are reported next to their raw property values
(`*_raw`). The only remaining [INFERENCE] is that the *property* value space equals the *getter*
value space of the same name — the live sample agrees (`0/0/0` on a fully decrypted volume, and the
property and getter values are identical). Unmapped raw numbers produce a typed field of `None`
with the raw number preserved.

**Deliberately not used.** `GetVersion` (declared `Unknown/Vista/Win7`, answered `0` — a
BitLocker-format generation, not a state), `GetHardwareEncryptionStatus`, `GetSuspendCount`,
`GetKeyProtectors`, `GetKeyProtectorNumericalPassword`, `SaveExternalKeyToFile`,
`BackupRecoveryInformationTo*`, `ProtectKeyWith*`, `Enable/DisableKeyProtectors`, `UnlockWith*`,
`Encrypt`, `Decrypt`, `Lock` — every one of them either exposes key/recovery material or changes
state, and none is needed to report the posture. No recovery password or key protector is read.

Access: the namespace is administrator-only. A denied query is reported as *unavailable with the
mapped permission error*, and an empty result is reported as "no encryptable volumes", which is a
successful observation, not an error.

## 5. Rejected candidates

| candidate | why not used |
|---|---|
| `Confirm-SecureBootUEFI` (PowerShell cmdlet) | It reads the same `SecureBoot` variable; using it would spawn PowerShell and add a shell surface for no extra authority. Kept as an independent recon cross-check only (`True`). |
| `msinfo32` "Secure Boot State" / `systeminfo` | Text scraping of a UI/report tool; no stable machine-readable contract. |
| `Win32_DeviceGuard` (VBS/HVCI) | A different posture (virtualization-based security), not the firmware Secure Boot variable; out of this item's scope. |
| UEFI `db`/`dbx` signatures | Key-database contents; reading them is not needed to state Secure Boot state, and they live in a different GUID (§2). |
| `Get-Tpm` / `Win32_Tpm` methods (`IsReady`, `IsLockedOut`, …) | Method invocation on the TPM provider; the property subset already answers the state question, and keeping the verb method-free removes any proximity to ownership/clear paths. |
| BitLocker module cmdlets (`Get-BitLockerVolume`, `manage-bde -status`) | Both are wrappers over `Win32_EncryptableVolume` methods; parsed text or a second provider surface adds nothing over the property query. |
| `TPM` WMI `root\CIMV2\Security\MicrosoftTpm` write methods | Out of scope by definition (no key operations). |

## 6. Implementation contract

`security status` is read-only and reports three independent components. Each component names the
source that produced its state; a failed read is `unavailable` with the underlying reason and never
collapses into "off"/"absent".

- `secure boot`: `enabled` / `disabled` / `not_uefi` / `unavailable`
  - source: `uefi_firmware_variable` (`{8be4df61-…}:SecureBoot`), or `get_firmware_type` when the
    machine boots legacy BIOS.
  - corroboration fields: `setup_mode`, `platform_key_present` (+ byte length), and
    `registry_secure_boot_enabled` from `HKLM\SYSTEM\CurrentControlSet\Control\SecureBoot\State`.
- `tpm`: `present` / `absent` / `unavailable`
  - source: `win32_tpm`; fields: `enabled`, `activated`, `owned`, `spec_version`,
    `manufacturer_id`, `manufacturer_id_text`, `manufacturer_version`,
    `manufacturer_version_full`, `manufacturer_version_info`, `physical_presence_version`.
- `bitlocker`: `observed` (with `volumes[]`, possibly empty) / `unavailable`
  - source: `win32_encryptable_volume`; per volume: `drive_letter`, `device_id`,
    `persistent_volume_id`, `protection_status`, `conversion_status`, `encryption_method`,
    `volume_type`, `is_volume_initialized_for_protection`, each with its raw number and the derived
    map above, plus the two read-only getter results `lock_status` (`Unlocked`/`Locked`) and
    `encryption_percentage`. A getter that fails leaves its field `None` and its reason in the
    volume's `detail`; nothing becomes `Unlocked`/`0` by default.
  - the number of volumes interrogated is bounded (`MAX_BITLOCKER_VOLUMES`), so a machine with many
    volumes cannot turn one read into an unbounded sequence of method calls; the truncation is
    reported, not silently dropped.

Channel capability: `channel.security` is recorded from a live probe of all three reads with
`Available` (all three observed), `Limited` (some observed, with the failing reasons), or
`Unavailable` (none), so `vantage capability` cannot claim a channel that did not answer.

Registry wiring: `CommandId::SECURITY_STATUS` (`security.status`), `ServiceRole::Security`
(`as_str = "security"`, mapped to `channel.security` in the capability matrix), and
`Dependency::Win32FirmwareEnvironment` (required) plus `Dependency::Win32Registry` (optional — the
registry mirror is corroboration, not the state source), next to the already-required
`Dependency::PublicWin32Cim`. This closes the two gaps recorded for this row in
`docs/evidence/capability-audit-20261105.md` §3.8 ("no recording site; `ServiceRole::Security`
unmapped"); `docs/08-architecture.md` already declares the command in the tree.

## 7. Acceptance

Read-only on the target; expected on `tb14p-ezra`:

```
vantage security status            # human: secure boot enabled, tpm present (INTC 2.0), C: protection off
vantage --json security status     # structured report with per-component source + raw values
```

Negative controls:

- non-elevated run: Secure Boot read may still answer, TPM/BitLocker namespaces return
  `WBEM_E_ACCESS_DENIED` → those components `unavailable` with a permission reason, exit still 0
  for the read;
- legacy-BIOS machine: `secure boot: not_uefi`, no Secure Boot variable read attempted;
- machine without the TPM provider: `tpm: unavailable` with the namespace HRESULT, not `absent`;
- command must never write: no `SetFirmwareEnvironmentVariable*`, no `Win32_Tpm` method, no
  BitLocker state-changing method (`Encrypt`/`Decrypt`/`Lock`/`ProtectKeyWith*`/`UnlockWith*`/
  `Disable*KeyProtectors`/key-protector or recovery readers), no registry write. The only
  `Win32_EncryptableVolume` methods allowed are the four read-only getters in §4.

## 8. Open items

- The four getters are invoked per volume through the same `WmiTransport::invoke_instance` path used
  by the BIOS assistant. A provider that rejects the empty in-params instance leaves the getter
  fields `None` with its HRESULT recorded — that failure mode is untested on a machine whose volume
  is actually encrypted (this target's only volume is fully decrypted).
- `GetFirmwareType` reports `Uefi`/`Bios`; an `Unknown` return (API failure) is treated as
  unavailable rather than guessing the firmware mode.
