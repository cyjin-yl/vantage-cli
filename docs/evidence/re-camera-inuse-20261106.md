# Camera in-use / consent read — contract and implementation record — 2026-11-06

Scope: a **read-only** `privacy camera inuse` that reports what Windows'
Capability Access Manager has *recorded* about camera consent and last use, per
application, from the registry. It answers no other question: it is explicitly
**not** proof that a camera is streaming now.

Written before any source edit. Related records: channel recon
[`re-privacy-osd-20261105.md`](re-privacy-osd-20261105.md), the runtime/PnP
camera switch [`re-privacy-cam-impl-20261105.md`](re-privacy-cam-impl-20261105.md)
(`privacy cam on|off|status` — devnode enable/disable), and the persistent BIOS
`IntegratedCamera` layer in `04-peripherals.md` §4.1.

## 1. Contract recovery — live, read-only registry probe

Target: reference machine `tb14p-ezra` (Windows 11, ThinkBook 21VG).
One batched probe, no writes, no service calls:

```bat
reg query "HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam" /s
reg query "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam" /s
reg query "HKLM\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy" /s
```

Observed structure (trimmed to the records that carry the contract; every key
named below appears in the probe output verbatim):

```
HKEY_CURRENT_USER\...\ConsentStore\webcam
    Value                         REG_SZ     Allow
    LastSetTime                   REG_QWORD  0x…
HKEY_CURRENT_USER\...\ConsentStore\webcam\Microsoft.WindowsCamera_8wekyb3d8bbwe
    Value                         REG_SZ     Allow
    LastSetTime                   REG_QWORD  0x1dcad31e5023476
    LastUsedTimeStart             REG_QWORD  0x1dd2e18bfdecedf
    LastUsedTimeStop              REG_QWORD  0x1dd2e18c89f9036
    LastUserAnnotatedLabel        REG_DWORD  0x2
    PersistedInDatabase           REG_DWORD  0x1
HKEY_CURRENT_USER\...\ConsentStore\webcam\NonPackaged
    Value                         REG_SZ     Allow
    LastSetTime                   REG_QWORD  0x1dd2ae7a4b475f0
HKEY_CURRENT_USER\...\ConsentStore\webcam\NonPackaged\C:#Program Files#Google#Chrome#Application#chrome.exe
    LastUsedTimeStart             REG_QWORD  0x1dcd0a370206346
    LastUsedTimeStop              REG_QWORD  0x1dcd0a3833d2abd
HKEY_LOCAL_MACHINE\...\ConsentStore\webcam
    Value                         REG_SZ     Allow
    LastSetTime                   REG_QWORD  0x1dc908952686dd2
HKEY_LOCAL_MACHINE\...\ConsentStore\webcam\NonPackaged\C:#Windows#System32#svchost.exe
    LastUsedTimeStart             REG_QWORD  0x1dd6084b41a8ceb
    LastUsedTimeStop              REG_QWORD  0x1dd6084b585bcd4
```

`HKLM\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy` **does not exist** on this
machine (`ERROR: The system was unable to find the specified registry key or
value`). An absent policy key is therefore the *observed* state here, not a
hypothetical.

Facts the probe establishes on this machine:

| Fact | Evidence |
|---|---|
| Two stores are populated: `HKCU` (invoking user) and `HKLM` (machine-wide records) | both roots list `Value`+`LastSetTime`, and both carry per-app subkeys |
| Per-app consent text is a `REG_SZ` named `Value`, spelled `Allow` | `webcam`, `…\Microsoft.WindowsCamera_8wekyb3d8bbwe`, `…\NonPackaged` |
| Per-app use stamps are `REG_QWORD` `LastUsedTimeStart` / `LastUsedTimeStop` | every per-app subkey |
| Non-packaged programs live under `\NonPackaged\<path with '#' for '\'>` | `C:#Program Files#Google#Chrome#Application#chrome.exe` |
| A record can carry use stamps **without** a consent `Value` | `webcam\Microsoft.BioEnrollment_cw5n1h2txyewy`, every `\NonPackaged` child |
| A container can exist with **no** values at all | `HKLM\…\webcam\NonPackaged` |
| `LastUserAnnotatedLabel` (`REG_DWORD`, `0x2`) and `PersistedInDatabase` (`REG_DWORD`, `0x1`) accompany records | per-app subkeys |

`LastUsedTime*` and `LastSetTime` decode as Windows FILETIME (100 ns ticks since
1601-01-01 UTC). Decoding the observed ticks lands in the machine's own history,
which corroborates the interpretation:

| Stored ticks | Decoded (UTC) |
|---|---|
| `0x1dd2e18bfdecedf` / `0x1dd2e18c89f9036` (`Microsoft.WindowsCamera`) | 2026-08-17T07:19:32.4125919Z / 2026-08-17T07:19:47.0976054Z |
| `0x1dd2e1e67e17bcd` / `0x1dd2e253de73ad7` (`Feishu.exe`) | 2026-08-17T08:00:01.7710029Z / 2026-08-17T08:48:57.8214615Z |
| `0x1dd6084b41a8ceb` / `0x1dd6084b585bcd4` (`HKLM … svchost.exe`) | 2026-10-20T11:18:16.4571371Z / 2026-10-20T11:18:18.8373204Z |

Authority for the consent spellings and the `#`-encoded `NonPackaged` layout is
this live registry plus published DFIR analyses of the Capability Access Manager
store (Zachary Stanford, *Can You Track Processes Accessing the Camera and
Microphone on Windows 10?*, DFIR Review 2020; the Velociraptor
`Windows.Registry.CapabilityAccessManager` artifact). **Microsoft does not
document this registry layout as a supported interface** — it is an
implementation detail of the consent store, which is exactly why the report says
"as recorded by CapabilityAccessManager" instead of presenting it as an OS API.

Policy semantics come from the documented policy, not from a probe: the Privacy
Policy CSP `<Policy>_LetAppsAccessCamera` ACPI value is
`0 = User in control`, `1 = Force allow`, `2 = Force deny`
(<https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-privacy>).
Group-policy lists for the same policy are `LetAppsAccessCamera_ForceAllowTheseApps`
and `LetAppsAccessCamera_ForceDenyTheseApps` (`REG_MULTI_SZ`). None of those keys
exist on this machine today, so the shipped code must report the policy key as
absent rather than assume "user in control".

## 2. Why this is not a live-streaming indicator

Stated in the product itself (`as_recorded` in JSON, plus a banner line):

1. Proven: each app record holds exactly one start/stop pair, and every pair in
   the §1 probe is closed (`start < stop`). `[INFERENCE]` The pair is the *last*
   session's write, overwritten by the next one — the store has no timeline and no
   concurrency, so absence of an app is absence of a record, not proof the app
   never opened a camera.
2. Records can be stale or partial: `Microsoft.BioEnrollment…` carries use stamps
   but no `Value`; `HKLM\…\webcam\NonPackaged` exists with no values; per-app
   `Value` may be missing entirely.
3. Observed: both containers record `Allow`, and no record in §1 carries `Deny` or
   `PromptOnFirstUse`. Those two spellings come from the store's value domain as
   published in the DFIR analyses above (and from the Windows privacy page, whose
   three states they name) — this machine exercises only `Allow`, so the other two
   are accepted and rendered, not demonstrated here. `[INFERENCE]` The OS resolves
   consent with policy this reader does not evaluate, so the report keeps each
   store separate: on a machine where the stores disagree, the output shows two
   answers instead of an averaged verdict.
4. Sandboxed/elevated/session-0 captures and hardware paths that bypass the
   consent broker are known blind spots of this artifact class (DFIR Review,
   above). A streaming camera with an empty store is possible; a store entry with
   `start > stop` is a *record*, not an observation of the device.

## 3. Design (implemented after this record)

### 3.1 Pure types and decoding — `crates/lctrl-core/src/privacy.rs`

Additive only; nothing existing in that module changes.

- `CameraConsentScope { CurrentUser, CurrentUserNonPackaged, Machine, MachineNonPackaged }`
  — the four containers actually observed, reported separately. A machine `Deny`
  is never merged into an app `Allow`.
- `CameraConsent { Allow, Deny, PromptOnFirstUse }` + `CameraConsent::parse`:
  exact documented spellings only. Anything else is **not** coerced; the raw text
  is preserved in `consent_raw`.
- `CameraStamp { filetime, utc }` + `filetime_to_rfc3339(u64)`: FILETIME → RFC
  3339 UTC at 100 ns precision (Howard Hinnant civil-date algorithm, the same
  approach already used by `lctrl-hal-win::update_history::ticks_to_rfc3339`,
  shifted to the 1601 epoch instead of the .NET 0001 epoch).
- `decode_non_packaged_path(&str) -> Option<String>`: `#` → `\` **only** for
  names that look like an encoded absolute path (`X:#…` or `##…` for UNC).
  Anything else returns `None`, and the entry keeps the stored subkey name
  verbatim in `key` — undecodable names are reported raw, never dropped. Names
  containing a literal `#` are decoded with that `#` turned into `\`; the
  encoding carries no escape for a literal `#`, so the raw `key` is what proves
  what was stored (disclosed, not hidden).
- `CameraConsentEntry::new(...)`: the single mapping used by the platform reader.
  `in_use` is `start > stop` after both are required non-zero — the rule is also
  emitted as text (`in_use_rule`) so the output states what it computed.
- `CameraConsentReport { as_recorded, in_use_rule, scopes, policy }`,
  `CameraConsentScopeReport { scope, present, consent, consent_raw, last_set, entries, notes }`,
  `CameraPrivacyPolicy { present, let_apps_access_camera, let_apps_access_camera_value, force_allow, force_deny, notes }`.
  Both `notes` vectors carry read failures (per-subkey for a container, per-value
  for the policy) so one unreadable record cannot silently truncate the report.

### 3.2 HAL trait — `crates/lctrl-hal/src/peripherals.rs`

`PrivacyControl::camera_consent()` with a defaulted
`Unsupported { feature: "privacy.cam.inuse" }` body, the same shape as the
existing `camera_devices`. Linux and every test fake stay honest without edits;
no runtime path is promoted.

### 3.3 Windows reader — `crates/lctrl-hal-win/src/camera_consent.rs` (`#[cfg(windows)]`)

`RegOpenKeyExW(…, KEY_READ, …)` only, on
`SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam`
under `HKEY_CURRENT_USER` and `HKEY_LOCAL_MACHINE`:

- missing key → `present: false` for that scope (not an error, not a verdict);
- otherwise read the container's `Value`/`LastSetTime`, enumerate its subkeys
  with `RegEnumKeyExW`, read `Value`, `LastSetTime`, `LastUsedTimeStart`,
  `LastUsedTimeStop` per subkey via `RegQueryValueExW` typed as `REG_SZ` /
  `REG_QWORD`;
- a subkey named `NonPackaged` is opened as its own container
  (`…NonPackaged` scope) and its children become entries in that scope;
- one subkey that fails to open or read becomes a `notes` entry; the rest of the
  list still reports;
- `LastUserAnnotatedLabel` / `PersistedInDatabase` are not interpreted (their
  meaning is not established) and are not reported.

The policy key `SOFTWARE\Policies\Microsoft\Windows\AppPrivacy` is read with the
same `KEY_READ` open; absent ⇒ `policy.present: false` and no inferred value.
`LetAppsAccessCamera` is reported as the documented enum **and** its raw
`REG_DWORD`, so an unknown value is visible instead of being dropped;
`_ForceAllowTheseApps` / `_ForceDenyTheseApps` are reported as `REG_MULTI_SZ`
lists when present. A policy value that exists with an unexpected type becomes a
`policy.notes` entry instead of failing the whole read (the consent store is the
answer the operator asked for; a mistyped policy value must not erase it).

`WindowsRuntimeDevices` (`runtime_devices.rs`) forwards
`PrivacyControl::camera_consent` to this reader. It deliberately does **not**
touch the PnP camera group: the registry answer must remain available when the
devnodes are disabled — the exact state the sibling `cam on|off` command creates.

### 3.4 CLI — `crates/vantage-cli/src/lib.rs`

`privacy cam inuse` (and `privacy camera inuse`, the existing subcommand alias).
New `PrivacyCamAction::InUse`:

- read-only: absent from `command_is_mutating` (which names `On | Off` only),
  `toggle()` returns `None`;
- `--runtime` / `--persistent` are rejected with `invalid_argument`, exactly like
  `status`, because this read spans every store and layer;
- human output prints the `as_recorded` banner, the in-use rule, a line per
  store (present/absent, container consent, `LastSetTime`), one line per app
  (`*` marks a record satisfying the in-use rule, `!` marks a read note), the
  final in-use count restated against the caveat, and the policy block only when
  the key exists;
- JSON is the report struct itself, so `in_use` is accompanied by its rule text;
- `privacy` service absent → `Unsupported { feature: "privacy.cam.inuse" }`, never
  an empty success.

No capability-registry row is added (`docs/evidence/re-privacy-cam-impl-20261105.md`
§3.2 precedent: a layer-flagless privacy read maps to no `CommandSelector`).

## 4. Authority and lifecycle

- Authority: Windows Capability Access Manager store as materialised in
  `HKCU`/`HKLM` on this machine (§1); policy semantics from the Privacy Policy
  CSP. No Lenovo binary is involved — this is an OS artifact, not a vendor channel.
- Lifecycle: `HKCU` stores belong to the invoking user and follow that profile;
  `HKLM` stores are machine-wide. Values are rewritten by the OS whenever an app
  is granted/denied or uses the camera; the report is a snapshot of the last
  write, and each field is independently meaningful.
- This change reads only. No `RegSetValue*`, no `CM_*DevNode`, no device handle,
  no `MediaCapture` session.

## 5. Acceptance — designed, NOT run by this worker

This worker ran no build, test, formatter, linter, or target program.

1. `cargo check -p lctrl-hal-win --target x86_64-pc-windows-msvc`
   (new `#[cfg(windows)]` module + trait forwarder).
2. `cargo test -p lctrl-core` — new unit tests in `privacy.rs`:
   - `filetime_decodes_to_the_utc_stamp_it_encodes`: `filetime_to_rfc3339` on
     §1's ticks reproduces the §1 decoded table (including
     `0x1dd2e18bfdecedf` → `2026-08-17T07:19:32.4125919Z`), the Unix epoch maps
     exactly, and zero/pre-1970 ticks stay `None`;
   - `non_packaged_names_decode_only_when_they_are_encoded_absolute_paths`:
     `decode_non_packaged_path` decodes
     `C:#Program Files#Google#Chrome#Application#chrome.exe` →
     `C:\Program Files\Google\Chrome\Application\chrome.exe`, UNC `##srv#share#a.exe`
     → `\\srv\share\a.exe`, and returns `None` (raw preserved) for a name that is
     not shaped like an encoded absolute path (`app.exe`, `#Program Files#app.exe`,
     `C:#` with nothing after it);
   - `consent_parses_only_the_three_documented_spellings`: `CameraConsent::parse`
     accepts exactly `Allow`/`Deny`/`PromptOnFirstUse`, and rejects e.g. `allow`
     (which then stays in `consent_raw`);
   - `in_use_requires_two_non_zero_stamps_with_start_after_stop`: the rule is true
     for `start > stop` with both non-zero, and false for `start < stop`, equal
     stamps, `stop = 0`, `start = 0`, and either side missing;
   - `entries_keep_unknown_consent_and_undecodable_names` (unknown `Value` text
     survives in `consent_raw`; a non-decodable `NonPackaged` name keeps `key` and
     leaves `path: null`), `policy_values_map_only_documented_numbers`,
     `report_keeps_scopes_separate_and_counts_in_use_records`.
3. `cargo test -p vantage-cli --test cli_contract` — new
   `camera_inuse_reports_each_consent_store_separately_without_mutation`: a fake
   privacy service returns a report and panics on every device call, so the test
   proves no PnP read and no write; it asserts the `as_recorded` text, the
   `in_use_rule` text, three scopes kept distinct (`CurrentUser` Allow,
   `CurrentUserNonPackaged` with no consent text, `Machine` Deny), the decoded
   `path`, `entries[0].in_use`, `policy.present == false` with the policy value
   absent, and the human caveat lines.
4. `cargo test -p vantage-cli --test cli_contract` —
   `camera_inuse_accepts_the_hyphenated_spelling` (`privacy cam in-use` parses to
   `Cam { action: InUse, .. }`), `camera_inuse_rejects_a_layer_flag_without_reading_anything`
   (`--runtime` fails `InvalidArgument` before the fake service is touched), and
   `camera_inuse_without_the_channel_is_unsupported_not_empty_success`
   (`Unsupported { feature: "privacy.cam.inuse" }`).
5. Target (unrun, needs the reference machine):
   - `vantage privacy camera inuse --json` → `as_recorded` starts with "as
     recorded by CapabilityAccessManager", `in_use_rule` is the literal rule,
     `policy.present == false`, `scopes[].scope` covers `current_user` /
     `current_user_non_packaged` / `machine` / `machine_non_packaged`, and the
     `current_user_non_packaged` entries carry `path` for the `C:#…` keys.
   - Cross-check against a second, independent read of the same registry
     (`reg query …` before and after): identical values, proving the command
     wrote nothing and decoded correctly.
   - **Negative control:** freeze a known stamp pair by comparing two consecutive
     runs seconds apart — values must be byte-identical (no self-mutating read).
   - **Negative control:** `privacy camera inuse --runtime` exits `2`
     (`invalid_argument`) and prints no store data.
   - **No-proof check:** with the camera devnodes disabled by
     `privacy cam off --runtime`, `inuse` still reports the stores (registry read
     does not depend on PnP state) — and must not claim any live use.
6. `cargo run -p vantage-cli -- privacy cam inuse` on Linux must fail
   `Unsupported (privacy.cam.inuse)` — no fabricated empty report.

## 6. Open items / explicit non-goals

- No WinRT `MediaCapture` / `CameraOcclusionInfo` session (see the sibling
  record §4); no device open, so "is it streaming *now*" remains unanswered by
  design.
- `LastUserAnnotatedLabel` and `PersistedInDatabase` are recorded by Windows but
  their semantics are unestablished; they are not surfaced.
- The `Value` inheritance question (does a child without `Value` inherit the
  container's `Allow`?) is **not** resolved by the registry text; the reader
  reports the child's `consent` as absent and never inherits.
- `HKLM\…\webcam\NonPackaged` exists with no values on this machine; the report
  shows `present: true` with an empty `entries` list and no consent, which is the
  honest reading.
