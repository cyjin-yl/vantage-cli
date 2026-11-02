# Vantage WebView2/CDP investigation — TB14P-EZRA, 2026-11-02

## Technology correction

This installed Vantage is **not Electron**. Package `E046963F.LenovoCompanion` version `10.2608.43.0` is Store-signed MSIX / desktop bridge; native `LenovoVantage.exe` hosts downloaded React UI bundles in **Microsoft Edge WebView2**. The package contains no Electron/Chromium payload (`app.asar`, `node.dll`, Electron resource packs, etc.). WebView2 runtime 156.0.4314.58 is installed.

Read-only observations on 2026-11-02: `LenovoVantage.exe` PID 76016 was a windowless preload process in session 0. It had no CDP listener and no WebView2 children; the browser environment is created lazily when the UI is shown. No process/service/task/registry state was changed during this investigation.

## CDP entry

The applicable mechanism is not an Electron CLI argument. WebView2 reads this process environment variable when creating its browser environment:

```
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222
```

Changing that variable in a remote shell does not affect the already-running preload instance. A one-time Vantage relaunch with this process-scoped environment is required before `http://127.0.0.1:9222/json` can exist. Machine-wide WebView2 policy is not appropriate: it would affect unrelated applications.

The packaged application also has a hardwired WebView2 user-data directory and single-instance behavior. A copied executable with `--user-data-dir` is therefore not a non-conflicting alternative. No such attempt was made.

## Application-level read oracle

The React UI calls its native host through:

```
chrome.webview.hostObjects.vantageAPI.Shell.*
```

The preserved bundle includes contract/command dispatch, for example:

```
contract:"SystemManagement.BatteryMgmt",command:"Get-BatteryChargeMode"
```

With CDP enabled, read the same contract values and the visible DOM before/after a CLI write. Do not treat the UI cache or a returned command success alone as proof: retain the independent driver/registry/vendor readback as a separate receipt.

Relevant pages/settings to locate, not assumed API signatures:

- Power: charge mode, night charge, USB continuous power.
- Audio: Dolby enable/profile, smart noise-cancel mode.

## No-restart fallback

When the user has Vantage open, UI Automation can read its control tree without changing application state. This is sufficient for visible state assertions, but only while UI exists. `VantageMCPAddin` is present but disabled/not running; enabling it would be an in-app state change and was not done.

## Consent boundary

The owner requested CDP, but the initial Electron premise is factually wrong for this version. Before restarting the windowless preload instance and opening Vantage UI, obtain explicit approval for that concrete side effect. If declined, use UI Automation while the user has Vantage open, and do not claim CDP acceptance.
