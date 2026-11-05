# UDC mTLS credential hunt — 2026-11-05

Question: can `vantage update download/install` authenticate to `api_mtls_uri`
with the machine's own UDC credential, or is download/install permanently
blocked without reverse-engineering the key service?

## Findings

**No client certificate exists in the Windows cert stores.**
`Cert:\LocalMachine\My` and `Cert:\CurrentUser\My` both enumerate **zero**
certificates — the store is empty, so there is no exportable PKCS#12/PEM the
CLI could present. No Lenovo/UDC/device-subject cert in `Root`/`CA` either.

**UDC's auth is service-held, not file-held.**
TVSU ships `Lenovo.CertificateValidation.dll`, `SecureBlackbox.SSLClient.dll`,
and `Microsoft.IdentityModel.*.dll` + `System.IdentityModel.Tokens.Jwt.dll`.
The credential is brokered by `LenovoPCMKeyService.exe` (a .NET service). The
`*.exe`/`*.dll` that open cert stores (`CertOpenStore`,
`CertFindCertificateInStore`: `LAVService.exe`, `LenovoAppupdate.exe`,
`LenovoPcManagerService.exe`, `PCMModulesUpdate.exe`, `ServiceAssist.exe`,
`WSServicePlugin.dll`) do so for *server* validation / pinning
(`certPinningError` registry value exists), not to fetch a client cert for a
foreign process.

**The endpoints are token/mTLS-gated — 401 without the credential.**
Probing the real `manifest_url`/`download_url` from the synced catalog
(`api.cnno1.uds.lenovo.com/core/application-management/lcp/apis/v1/apps/...`)
with a bare HEAD request:
- TLS handshake succeeds (the `curl-ca-bundle.crt` server chain validates).
- Server returns **401 Unauthorized** — the client credential is required and
  is not present in our request.

**`Lenovo.DriverUpdate.Client.dll` is the legacy TVSU downloader**, not the
UDC path — its strings are `download.lenovo.com`, Win32_PnPSignedDriver, INF
signature verification, IBM SystemUpdate registry keys. The JWT/mTLS
acquisition lives in `LenovoPCMKeyService` + the SecureBlackbox SSL client.

## Verdict

`update.download`/`install` are **transport-gated, not implementation-missing**.
The client credential is a device-bound JWT/mTLS assertion issued through
`LenovoPCMKeyService`; it is not a file or store cert a pure-Rust client can
present without reverse-engineering that service's key custody. The honest CLI
surface is:
- `update check` — **implemented**: evaluates the synced `LastUpdateConfig`
  catalog against the device's MTM/org/applicability tags (live: 3 packages).
- `update download`/`install` — stay `Unsupported`, reason: UDC requires a
  service-brokered client credential that is not extractable to a foreign
  process; reverse-engineering `LenovoPCMKeyService` is out of scope and would
  not survive a service update.

Evidence files (target-side scratch, not committed): `mtls.txt`, `mtls2.txt`,
`mtls3.txt`, `mtls4.txt`, `probe_api.txt` under `C:\Users\ezra\`.
