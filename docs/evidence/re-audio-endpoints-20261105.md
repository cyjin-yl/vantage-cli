# Audio endpoints — MMDevice enumeration and endpoint volume — 2026-11-05

Scope: recover a real source for the OS audio endpoint inventory and endpoint
volume/mute on TB14P-EZRA (21VG, `10.0.26100`), then decide what the CLI may
implement. Read-only target recon only; no volume/mute was written and no
service was started, stopped, or reconfigured in this pass. The Dolby
(`ncalrpc:[DaxRpcEndpoint]`) and Elevoc (`ncalrpc:[ElevocRpcEndpoint]`) RPC
paths were not touched — they are a separate surface (`re-audio-write-20261105.md`).

## 1. Conclusion

| Question | Answer | Basis |
|---|---|---|
| Where does the endpoint inventory come from? | **OS COM:** `IMMDeviceEnumerator` (`CLSID_MMDeviceEnumerator` = `MMDevApi.dll`) → `EnumAudioEndpoints` → `IMMDeviceCollection` → `IMMDevice` | §3, §4 |
| Where does the friendly name come from? | `IMMDevice::OpenPropertyStore(STGM_READ)` → `IPropertyStore::GetValue(PKEY_Device_FriendlyName)` | §3.2, §4.2 |
| Where does volume/mute come from? | `IMMDevice::Activate(IID_IAudioEndpointVolume)` → `GetMasterVolumeLevelScalar` / `GetMute` / `GetChannelCount` / `GetVolumeStepInfo` / `QueryHardwareSupport` | §4.3 |
| Is there a reversible documented write? | **Yes:** `SetMasterVolumeLevelScalar(f32 in [0,1], NULL)` and `SetMute(BOOL, NULL)`, both with a same-interface readback | §5 |
| Is the registry an alternative source? | **No** for this surface: the registry has no friendly-name value and no volume value, its `DeviceState` word carries private audio-service flags, and it is a stale superset | §3 |
| Legacy winmm (`Drivers32` `wave*`/`mixer*`) instead? | No: one generic mapper device (`wdmaud.drv`), device index only, no endpoint identity/mute/channel data | §3.3 |

Nothing here is vendor code. Both the CLSID and all interface IDs are Windows
OS components (`MMDevAPI.dll`), so the implementation loads no Lenovo payload
and needs no vendor service.

## 2. Binaries (hashed on target)

| File | Version | Size | SHA-256 |
|---|---|---|---|
| `C:\Windows\System32\MMDevAPI.dll` | `10.0.26100.8875` | 703552 | `FE8B4F2901E3DC45B6ECFBDC5AE555962432AFF08C231E54E9E79DEF716F3123` |
| `C:\Windows\System32\audioses.dll` (= `AudioSes.dll`, same file) | `10.0.26100.8875` | 1853920 | `95AF7710EC1A13252283B3740A3EFEEB7C014790F99DC9404D6AEC426E5C6C4E` |

Services at recon time: `AudioEndpointBuilder` Running/Automatic, `Audiosrv`
Running/Automatic, `RpcSs` Running/Automatic.

## 3. Why not the registry, and why not winmm

### 3.1 Endpoint tree

`HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\{Render,Capture}\{guid}`
held **32 render** and **28 capture** keys. Raw `DeviceState` words under
`Render`:

| raw `DeviceState` | count | low nibble (`DEVICE_STATE_*`) |
|---|---|---|
| `0x00000001` | 4 | ACTIVE |
| `0x00000004` | 13 | NOTPRESENT |
| `0x00000008` | 3 | UNPLUGGED |
| `0x21000004` | 12 | NOTPRESENT + private flag bits `0x21000000` |

The `0x21000000` bits are not part of `DEVICE_STATE_*`; the word is the audio
service's cache, not the MMDevice answer, and a consumer would have to mask the
low nibble to guess. `Get-PnpDevice -Class AudioEndpoint` reported only **16**
endpoints against the 60 registry keys — the registry keeps stale entries that
the MMDevice API (which asks `AudioEndpointBuilder`) does not return.

### 3.2 No friendly name, no volume value in the registry

For every ACTIVE render endpoint the `…\Properties` key contains
`{a45c254e-df1c-4efd-8020-67d146a850e0},2` (device description) but **not**
`{a45c254e-df1c-4efd-8020-67d146a850e0},14` (`PKEY_Device_FriendlyName`) — the
value is computed by the property handler, not stored. A scan for
volume-shaped value names on one active endpoint
(`{2a8e3cb6-b4cf-4f88-877d-0931eec0f848}`) found only the
`{9855c4cd-df8c-449c-a181-8191b68bd06c},0` blob; there is no master-volume or
mute value anywhere in the tree. The registry path therefore cannot satisfy
"friendly name + volume/mute" at all.

The `Properties` tree does confirm the property-key spelling this
implementation uses: the stored names are literal
`{a45c254e-df1c-4efd-8020-67d146a850e0},<pid>` strings (e.g. `,2` for
device description, `,24` for the container class), matching
`functiondiscoverykeys_devpkey.h`.

### 3.3 Legacy winmm

`HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Drivers32` exists and maps
`wave`, `wave1`…`wave8`, `mixer`, `mixer1`…`mixer8` **all** to the single
generic mapper `wdmaud.drv`. That API addresses one waveform device by index
and packs volume into a 16-bit word; it exposes no endpoint identity, no
per-endpoint mute, and no channel count, and it cannot report which endpoint is
the system default. Rejected.

## 4. COM contract

### 4.1 Registration and identity

```
reg query HKLM\SOFTWARE\Classes\CLSID\{BCDE0395-E52F-467C-8E3D-C4579291692E}\InprocServer32
  -> C:\WINDOWS\System32\MMDevApi.dll
```

Every GUID used by the implementation was found **byte-exact** inside
`MMDevAPI.dll` (little-endian `Data1/Data2/Data3`, big-endian `Data4`), which is
the check that the constants are the ones this OS build actually registers:

| Interface / key | GUID | file offsets |
|---|---|---|
| `IMMDeviceEnumerator` | `A95664D2-9614-4F35-A746-DE8DB63617E6` | `0x834c0`, `0x838d0` |
| `IMMDeviceCollection` | `0BD7A1BE-7A1A-44DB-8397-CC5392387B5E` | `0x83098` |
| `IMMDevice` | `D666063F-1587-4E43-81F1-B948E807363F` | `0x83448` |
| `IPropertyStore` | `886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99` | `0x82c00` |
| `IAudioEndpointVolume` | `5CDF2C82-841E-4546-9722-0CF74078229A` | `0x7c1e0`, `0x964a0` |
| `PKEY_Device_FriendlyName` fmtid | `A45C254E-DF1C-4EFD-8020-67D146A850E0` (pid 14) | `0x84380`, `0x84398`, `0x843b0` |

### 4.2 Enumerated method order

MIDL interpreter stubs carry no names, so the slot order is taken from the
Win32 metadata that ships the same headers (`windows` 0.61.3
`Win32::Media::Audio` / `Endpoints`, `Win32::UI::Shell::PropertiesSystem`,
which is generated from the SDK's `mmdeviceapi.h` / `endpointvolume.h` /
`propsys.h`). vtable slot numbers below are absolute (0 = `QueryInterface`):

| Interface | slot | method |
|---|---|---|
| `IMMDeviceEnumerator` | 3 | `EnumAudioEndpoints(EDataFlow, DWORD stateMask, IMMDeviceCollection**)` |
| | 4 | `GetDefaultAudioEndpoint(EDataFlow, ERole, IMMDevice**)` |
| `IMMDeviceCollection` | 3 | `GetCount(UINT*)` |
| | 4 | `Item(UINT, IMMDevice**)` |
| `IMMDevice` | 3 | `Activate(const IID&, CLSCTX, PROPVARIANT*, void**)` |
| | 4 | `OpenPropertyStore(STGM, IPropertyStore**)` |
| | 5 | `GetId(LPWSTR*)` (caller frees with `CoTaskMemFree`) |
| | 6 | `GetState(DWORD*)` |
| `IPropertyStore` | 5 | `GetValue(const PROPERTYKEY&, PROPVARIANT*)` |
| `IAudioEndpointVolume` | 5 | `GetChannelCount(UINT*)` |
| | 7 | `SetMasterVolumeLevelScalar(float, const GUID*)` |
| | 9 | `GetMasterVolumeLevelScalar(float*)` |
| | 14 | `SetMute(BOOL, const GUID*)` |
| | 15 | `GetMute(BOOL*)` |
| | 16 | `GetVolumeStepInfo(UINT* step, UINT* stepCount)` |
| | 19 | `QueryHardwareSupport(DWORD*)` |

Constants used: `eRender=0`, `eCapture=1`; `eConsole=0`; `STGM_READ=0`;
`CLSCTX_ALL=0x17`; `DEVICE_STATE_ACTIVE=1`, `DISABLED=2`, `NOTPRESENT=4`,
`UNPLUGGED=8`, `ALL=0xF`; `VT_LPWSTR=31`;
`ENDPOINT_HARDWARE_SUPPORT_VOLUME=1`, `_MUTE=2`;
`RPC_E_CHANGED_MODE=0x80010106`. `PROPVARIANT` is read as
`{vt:u16, wReserved[3], payload[16]}` (union at byte offset 8 on x64) and is
released with `PropVariantClear`.

### 4.3 What the surface can and cannot say

- `GetChannelCount` is the channel count of the endpoint's shared-mode mix
  format. It needs `Activate` to succeed, which is why a non-active endpoint
  reports **no** channel count instead of `0`.
- `GetVolumeStepInfo` returns `(step, stepCount)`. A hardware volume curve
  quantizes `SetMasterVolumeLevelScalar`, so a readback may legitimately land
  one step away from the request; the tolerance is derived from `stepCount`
  (`100 / stepCount` whole percent) instead of being invented.
- `QueryHardwareSupport` says whether the volume/mute is applied by the
  endpoint hardware or emulated; both are still writable through the same
  interface.
- The default endpoint is `GetDefaultAudioEndpoint(eConsole, flow)`, i.e. the
  console-role default the OS volume UI also uses. `eMultimedia` /
  `eCommunications` are different roles and are deliberately not merged into
  one "default" answer.

## 5. Write contract and its limits

`SetMasterVolumeLevelScalar` and `SetMute` are documented, reversible, and
read back through the same interface, so both verbs are implementable:

1. read previous state (`GetMasterVolumeLevelScalar`, `GetMute`,
   `GetChannelCount`, `GetVolumeStepInfo`, `QueryHardwareSupport`);
2. `--dry-run` returns that previous state and the requested one, and calls no
   `Set*` slot;
3. on commit, `Set*` then poll the readback (10 × 50 ms). Volume compares
   within the step-derived tolerance; mute compares exactly. A mismatch is
   `VerifyMismatch`, never a silent success;
4. `--auto-restore` replays the captured previous value and re-reads it with
   the same comparison.

Not proven by this pass: that a live commit moves the audible level and that the
readback equals the request on this specific endpoint. That requires the
target run in §8; the code path is written to fail loudly if it does not.

Scope note (deliberate): only the **master** volume and the endpoint-wide mute
are exposed. Per-channel volume (`SetChannelVolumeLevelScalar`) and the
`IAudioMeterInformation` peak meter are out of scope for this item — a
per-channel surface without a documented reason to pick a channel would be
invented API.

## 6. Implementation mapping

| Requirement | Source |
|---|---|
| endpoint model (`id`, `friendly_name`, `flow`, `state`, `channels`, `volume_percent`, `mute`, `default`) | `crates/lctrl-core/src/audio.rs` (`AudioEndpoint`, `AudioEndpointState`, `AudioDataFlow`) |
| volume/mute model + display | `crates/lctrl-core/src/audio.rs` (`EndpointVolume`) |
| scalar↔percent and step tolerance | `crates/lctrl-core/src/audio.rs` (`volume_percent_from_scalar`, `percent_to_scalar`, `volume_readback_tolerance_percent`) |
| HAL seam | `crates/lctrl-hal/src/audio_endpoints.rs` (`AudioEndpointControl`) |
| COM transport (seam + native) | `crates/lctrl-hal-win/src/audio_endpoints.rs` (`MmDeviceApi`, `NativeMmDeviceApi`, `WindowsAudioEndpoints`) |
| dry-run / readback / restore protocol | `crates/lctrl-hal-win/src/audio_endpoints.rs` `impl AudioEndpointControl for WindowsAudioEndpoints` |
| CLI verbs | `crates/vantage-cli/src/lib.rs` (`AudioCommand::{Endpoints,Volume,Mute}`, `execute_audio_endpoints`, `execute_audio_volume`, `execute_audio_mute`) |
| capability ids/rows | `crates/lctrl-core/src/command_capability.rs` (`AUDIO_ENDPOINTS`, `AUDIO_VOLUME`, `AUDIO_MUTE`), `crates/lctrl-hal/src/command_capability.rs` |
| live probe records | `crates/lctrl-hal-win/src/windows_hal.rs` (`channel.mmdevice`, `audio.endpoints`, `audio.volume`, `audio.mute`, plus the `channel.audio` join) |
| composition root | `vantage/src/main.rs` (`with_audio_endpoints`) |

Dependency hygiene: `windows-sys` 0.61 has **no** MMDevice/IAudioEndpointVolume
bindings (only the legacy `ACM`/`wave*`/`WAVEFORMATEX` surface), so the module
declares the `ole32` entry points and the six vtables it needs directly, the
same way the existing Dolby/Elevoc transports declare `rpcrt4`. No crate,
feature flag, or `windows`-crate dependency was added.

## 7. Live status

Observed read-only on the target (2026-11-05): 4 ACTIVE render and 5 ACTIVE
capture endpoints (per the service cache), 16 PnP `AudioEndpoint` devices,
`MMDevAPI.dll`+`audioses.dll` present with the IIDs above, both audio services
and `RpcS` running. No `vantage audio endpoints` run has happened yet: the
surface is new in this change and the parent owns build + target execution.

## 8. Suggested parent checks

1. **Read-only inventory:** `vantage --json audio endpoints` lists the 4 active
   render endpoints with non-empty friendly names, `channels` present, exactly
   one `default: true` per flow, and `vantage audio endpoints --all` grows the
   list to the non-present/unplugged endpoints from §3.1 (state fields must be
   the MMDevice state, not the `0x21000000`-flagged registry word).
2. **Independent cross-check:** compare each listed name with
   `Get-PnpDevice -Class AudioEndpoint` (`扬声器 (Realtek(R) Audio)` etc.) and
   confirm the `default` row equals the Windows sound-settings default output.
3. **Volume write:** capture `vantage --json audio volume`, run
   `vantage --dry-run audio volume 40` (must print previous 40-free plan and
   change nothing), then commit a value differing from the captured one and
   require the reported `actual` to be within the step tolerance; finish with
   `--auto-restore` and re-read.
4. **Mute write:** same shape with `audio mute on|off`, exact readback required.
5. **Negative controls:** (a) `audio volume 101` must fail `InvalidArgument`
   before any COM write; (b) `--device <unknown id>` must fail
   `ChannelUnavailable` (E_NOTFOUND path), never fall back to the default;
   (c) `audio volume` with no default render endpoint (headless session) must
   report `ChannelUnavailable`, never 0 %.
6. **No vendor coupling:** with `DolbyDAXAPI` and `ElevocService` treated as
   unavailable (e.g. a machine where `NativeDolbyRpc::load()`/`ElevocRpc` fail),
   `audio endpoints`/`volume`/`mute` must still work — they call only OS COM.

## 9. Open items (explicit)

0. **Registry binding kind**: the three rows
   (`CommandId::AUDIO_ENDPOINTS`/`AUDIO_VOLUME`/`AUDIO_MUTE`) use
   `FeatureBinding::Capability`, not `Unprobed`, because
   `WindowsHal::capabilities` records exactly those three ids
   (`crates/lctrl-hal-win/src/windows_hal.rs`). The probe counts an empty
   active enumeration as `Unavailable` with
   `the MMDevice API reported no active audio endpoint`, so the metadata never
   claims a surface the verbs cannot address (headless session, audio services
   stopped before `AudioEndpointBuilder` populated a device).
1. **Capability axis**: the volume/mute rows use `Readback::Required` because
   the implementation's commit path polls and compares before returning. It is
   not yet proven end-to-end on the target; if the parent wants the same
   conservative treatment as `audio.dolby`/`audio.noise-cancel`, change those
   two rows to `Readback::Unverified` — the axis lives in one place
   (`crates/lctrl-hal/src/command_capability.rs`).
2. **Percent rounding**: the CLI reports whole percent. A hardware volume
   curve can sit between two whole percent values, so the readback may differ
   by one step; the tolerance rule in §4.3 is the contract, not exact equality.
3. **Roles**: only the console-role default is flagged. Multimedia and
   communications role defaults are resolvable through the same slots and are
   not implemented because the item asked for one default flag.
4. **`IMMNotificationClient`**: no hotplug notification surface is exposed;
   every command re-enumerates, which is correct but not event-driven.
