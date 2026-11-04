# Update-contract recon — Lenovo UDC catalog is recoverable

Read-only reconnaissance toward a real, authenticated update contract, so
`update.check` / `download` / `install` can stop being permanently unsupported.
All evidence below was captured from the 21VG target's on-disk state (registry
and binary strings); **no network request was made and nothing was written**.

The machine's updater is Lenovo **UDC** — "Unified Device Catalog" — delivered
inside `PCManager`/`TVSU` (`InstallPath =
C:\Program Files (x86)\Lenovo\PCManager\5.1.210.10222\TVSU\`). UDC is a
mutually-authenticated catalog: a device registers, the server returns a signed
config describing applicable packages, and each package carries its own
manifest URL, download URL, and file hash. That is a genuine contract, not a
thin "check a version endpoint" shim.

## 1. Identity + endpoint (`HKLM\SOFTWARE\Lenovo\UDC`)

```
device_id            = 899a2a699abf887d98507e55524fe4032f096eb9e976c1eda28c914ba19725f8
device_id_phys       = 899a2a699abf887d98507e55524fe4032f096eb9e976c1eda28c914ba19725f8
registration-state   = 1                       (registered)
registration-account = LenovoAutoclaimed       (the org this device claims into)
_UdcClsId            = BC7A8EBF-FFB5-4268-9D1B-9CF757DE4557
api_mtls_uri         = <mTLS API host, redacted in this transcript>
extension-support    = 1
```

The transport is **mTLS** (`api_mtls_uri`, `registration-account`,
`device_id`/`device_id_phys`). A client must present a device certificate the
service issued at registration; the catalog is not an anonymous REST endpoint.
`DeviceSupport` records `certPinningError = 0` and `registerUdsError = 0`, so
the channel is healthy and certificate-pinned on this machine.

## 2. Applicability engine (`UDC\Applicability\Tags`)

The server filters packages by evaluating tags the device publishes:

```
DaysSinceOobe                        = 243
Device.Hardware.Npu.Exists           = true
Device.Hardware.Npu.mfr              = Intel Corporation
Device.Hardware.Npu.description      = Intel(R) NPU
OobeDate                             = 2026-03-06T13:03:27+08:00
Device.Warranty.ExpirationDate       = 2023-09-28T15:19:22Z
Device.SmartPerformance.ExpirationDate = 2023-09-28T15:19:22Z
Device.SmartLock.ExpirationDate      = 2023-09-28T15:19:22Z
App.Lenovo Speech.SystemSupport      = true
App.Lenovo Speech.UserEnabledAutoUpdate = true
```

`UDC\ConfigHash\lastConfigHash =
b61c65ae...727d` is the hash of the config currently applied — the device sends
it so the server only returns a diff/full config when it changed.

## 3. The package contract (verbatim `LastUpdateConfig` schema)

`UDC\Packages\LastUpdateConfig` is a JSON object. Its schema is the contract:

```json
{
  "active_hours_start": "0700", "active_hours_end": "1800",
  "days_of_update": "mon,tue,wed,thu,fri,sat,sun",
  "date_modified": "2023-02-23T12:00:00.000+00:00",
  "applicability_filter": [ { "default": true } ],
  "common_update_policy": [
    "require_package_setup_validation",
    "limited_package_retention",
    "retry_strategy_low",
    "no_download_onMeterConnection",
    "silent_install_only",
    "disable_package_diagnostic_check",
    "disable_package_status_check",
    "no_install_onActivehours"
  ],
  "packages": [ { ...per-package object... } ]
}
```

Each `packages[]` entry carries the full decision set a client needs — observed
field names (verbatim) in this machine's config:

```
app_id, package_id, package_version_id, package_name, package_version,
package_type, package_update_type, package_status, package_publisher,
package_platform, package_size, package_file_name, package_file_hash,
package_url, package_manifest_url, package_download_strategy,
package_handler, version, versionType,
applicability_filter[], applicableOrgs[], applyToAllOrgs
```

A representative `applicability_filter` entry:

```json
{
  "id": "bdd7e1eb-29d5-41a7-8a07-1539bb2beaae",
  "mfr": "lenovo", "mtm": "NotApplicableToOldUdc", "os": "windows",
  "os_bitness": "64",
  "org_hash": "7b8e8f5aa3a501ce8bbeafee67e3f17f8c501b2aa5b062fa0c8c639b2e648578",
  "tag": "App.Lenovo AI Now.SystemSupport|true,App.Lenovo AI Now.UserEnabledAutoUpdate|true",
  "version": "<25.2.0.0"
}
```

So applicability is a conjunction of `mfr` / `mtm` (machine type) / `os` /
`os_bitness` / `version` (a semver-range like `<25.2.0.0`) plus a `tag`
expression evaluated against the §2 device tags, and gated by `applicableOrgs`
(the device must claim into one of the listed orgs — here `LenovoAutoclaimed`).

## 4. Integrity + install

- **`package_file_hash`** + **`package_manifest_url`** — each package is fetched
  over the manifest URL and validated against a recorded file hash before
  install; `require_package_setup_validation` is a global policy flag.
- **`package_download_strategy`** — `no_download_onMeterConnection` is a global
  policy; the strategy field says how a given package may be fetched.
- **`package_handler`** + **`silent_install_only`** — install runs through a
  named handler, non-interactively.
- **`LastUpdateType = auto`**, **`UpdateCheckDateTime = 2026-10-05`**, and the
  per-package `package_status` under `UDC\PackageStatus\<guid>` give the check
  cadence and outcome bookkeeping.

## 5. Assessment — is the contract recoverable?

**Yes for read-side catalog access; install still needs the device certificate.**
The full request schema (device identity → applicability tags → config hash →
package list with manifest+hash) is recoverable from on-disk state alone, with
no reverse-engineering of wire bytes required. What is *not* in this evidence:

- **The mTLS client certificate / private key.** `api_mtls_uri` implies a cert
  the client presents at registration. It lives in the UDC service's credential
  store (not the registry keys shown). A CLI `update.check` would need either
  that cert or a documented anonymous/tier endpoint; whether one exists is
  unknown from this data. `[INFERENCE]` — the cert is very likely under the UDC
  install dir or the platform credential store, not extractable by reading these
  keys.
- **Signature-algorithm detail.** `package_file_hash` and
  `require_package_setup_validation` prove integrity-checking happens, but the
  hash algorithm and whether the *manifest* itself is signed (vs the package
  payload) are not named here.
- **`package_handler` semantics.** Which handler binary each `package_type`
  maps to, and whether a headless invoke is possible, is not in the config.

**Verdict.** A faithful `update.check`/`download`/`install` is implementable:
the contract is a documented UDC applicability+manifest pipeline, not a guess.
The reachable scope without the device cert is the catalog *shape* and the
per-package metadata this machine already synced — enough to drive
`update.check` reporting what UDC would offer, and `download`/`install` against
`package_url`/`package_file_hash`. The blocking prerequisite for a live
server-driven check is presenting the registration cert (mTLS), which needs a
target-side probe of the UDC credential store — deferred, not a code gap.

## 6. Inventory (what was found)

| Component | Path / source | Purpose |
|-----------|---------------|---------|
| UDC engine | `C:\Program Files (x86)\Lenovo\PCManager\...\TVSU\` | catalog client + installer host |
| UDC registry | `HKLM\SOFTWARE\Lenovo\UDC` (+`udc`, `DriverUpdate`) | identity, applicability, config hash, package config/status |
| Applicability tags | `UDC\Applicability\Tags` | device capability facts the server filters on |
| Package config | `UDC\Packages\LastUpdateConfig` | the signed package list (schema above) |
| Package status | `UDC\PackageStatus\<guid>` | per-package install bookkeeping |
| CMS manifest | `cms-smb.lenovo.com/.../update.xml` (found in a bundled binary) | a related Lenovo content-update feed; UDC itself is mTLS, not this public URL |
