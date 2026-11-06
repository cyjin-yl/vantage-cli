# Capability-matrix accuracy audit — 21VG target — 2026-11-05/06

Target: `tb14p-ezra` (ThinkBook 14 G8+ IPH, 21VG, Windows 11 26100, BIOS `SWCN22WW`).
Scope: every row of `COMMAND_REGISTRY` in `crates/lctrl-hal/src/command_capability.rs`, classified
against the live machine, plus the bounded `FeatureBinding::Unprobed → Capability` repairs that the
live probes prove. **No write, no service change, no driver call with side effects was issued.**
Compiler/IR, dispatch code, axes, dependencies and capability *ids* are untouched.

Registry snapshot audited (line numbers below are exact for this sha):

```
file    crates/lctrl-hal/src/command_capability.rs
bytes   83703
sha256  2e41712e1c0bceee924b92679f75118f1536236b0670ee79e994b838691f2394
rows    153  (CommandCapability::new entries)
```

The file is under concurrent edit by sibling workers (it grew 138 → 153 rows during this audit,
including `CPU_STATUS`, `EVENTS_LOG`, `FIRMWARE_STATUS`, `PANEL_INFO`, `AUDIO_ENDPOINTS`,
`AUDIO_VOLUME`, `AUDIO_MUTE`, `HOTKEY_STATUS`, `SECURITY_STATUS`, `STORAGE_STATUS`, `RADIO_*`).
This document is bound to the sha above; re-check before acting on a later tree.

Four further rows landed while the audit was being written — `USB_PORTS`, `TOUCHPAD_CAPS`,
`WIFI_STATUS`, `BLUETOOTH_LIST` — taking the file to 157 rows. They are classified in §11; they are
not part of the 153-row verdict tables. After the §7 change the file is

```
bytes   85078
sha256  aa88d27bcc43f201430fae4507f2155bb5a5aa1b91b47bc7acb951c5d6fd54ca   (157 rows: the 37 flips + 4 peer rows)
```

Raw evidence committed next to this note:

* `docs/evidence/target-capability-matrix-20261105.json` — verbatim `vantage --json capability`
  (134 rows; emitted by the 2026-11-05 14:23 build, see §1.2).
* `docs/evidence/target-capability-verb-probes-20261105.txt` — the per-verb live probe transcript
  (48 read-only invocations + 8 direct registry/WMI/CIM channel probes) used for the verdicts.

---

## 1. Method

### 1.1 What "live" means in this note

Three independent live sources were used, and every verdict says which one backs it:

| tag | source |
|---|---|
| **exec** | the verb itself was executed read-only on the target (`vantage --json …`, dry-run only where it is guaranteed write-free; no mutation was run at all) |
| **probe** | the feature id's value in a live `vantage capability` / `vantage info` run (the HAL's own `capabilities()` probe) |
| **direct** | an independent channel read that does not go through the CLI (registry `PowerSlider`, `root\wmi` class/instance census, SCM, ProgramData ledgers) |
| **doc** | an existing repo evidence file whose probe was itself live on this target (cited inline) |

### 1.2 Provenance limit (important)

The only binary on the target was `C:\Users\ezra\vantage-target\debug\vantage.exe`
(`vantage 1.0.0`, built 2026-11-05 14:23). It predates a subset of the current tree, so:

* its registry had **134** rows, the tree has **153**;
* `battery health`, `perf policy`, `perf gpu status`, `privacy cam status`, `hotkey status`,
  `fix rules`, `radio …`, `audio volume|mute|endpoints`, `panel info`, `cpu`, `events`,
  `security`, `storage` are **not invocable** with it (`EXIT=2`, "unrecognized subcommand");
* rows whose binding a sibling changed after 14:23 emit the *old* `feature_ids` in the matrix
  (`perf.fan.auto` shows `perf.fan.mode`, the tree now binds `perf.fan.mode.write`).

Consequence: the matrix JSON is authoritative for **channels** (its `CapabilitySet` came from a live
`capabilities()` probe) but not for row *shape*. Rows that the binary cannot express are marked
`unverified-by-this-audit` and never presented as proven. A rebuild is the parent's step (§10).

### 1.3 Verdict codes

| code | meaning |
|---|---|
| **TI** | truthful-Implemented: dispatch exists and the channel/verb worked live |
| **TG** | truthful-Implemented, live-gated: dispatch exists, the live probe records the feature `unavailable`/`limited` on this SKU, and the command refuses or narrows instead of claiming success |
| **TU** | truthful-Unsupported: dispatch refuses **and** the evidence shows the contract is absent on this SKU |
| **SU** | **stale-Unsupported**: dispatch refuses while the live channel does serve the verb (payload, §4) |
| **IP** | Implemented, but the bound feature id is recorded by **no** probe: `vantage capability` can only ever say `unverified` (payload, §5.2) |
| **RL** | cross-cutting defect: the row's `ServiceRole` has no `role_channel()` entry, so `service_presence` is `Unexposed` and the row can never be reported `available` (§6.2) |
| **LU** | limited-by-composition (`Dynamic`/`Composite` legs partly present → `limited`), honest |
| **EX** | `ServiceBinding::External` (handled by the platform binary) |
| **NV** | new row: a recording site exists in the current tree but this audit has no live probe value for it |

---

## 2. Summary counts

Every number below is countable from the registry and the two committed evidence files; the per-row
`TI/TG/TU/LU/EX` verdict is in §3 and the four rows that landed later are in §11.

| bucket | count |
|---|---|
| registry rows audited (snapshot `2e41712e…`) | 153 |
| dispatch `Implemented` / `Unsupported` / `External` (157-row file) | 129 / 24 / 4 |
| **payload A** — `Unsupported` whose channel is live | **1** (`UPDATE_CHECK`, §4) |
| **payload B** — `Implemented` with no recorded probe for its bound id | **10** (§5.2) |
| **defect** — `Implemented` bound to an unmapped `ServiceRole` | **10** (§6.2) |
| `Unprobed` rows in total (153-row snapshot) | 63 |
| … flipped `Unprobed → Capability` (§7) | **37** |
| … left `Unprobed` — no recording site exists | **19** |
| … left `Unprobed` — recording site exists, live proof insufficient | **7** |

Plus the four rows that landed after the snapshot (§11): 2 more stay `Unprobed` without a recording
site (`wifi.status`, `bluetooth.list`) and 1 more is blind (`usb.ports`) → final tallies over the
157-row file: `Unprobed` **65** = 37 flipped + 21 without a recording site + 7 without live proof.

Cross-cutting (not additional rows): **RL** affects the 10 `Implemented` rows of §6.2 (7 of them are
also blind) plus 3 `Unsupported` rows where the role is moot.

---

## 3. Per-command verdicts

`binding→live` is the declared binding and the live probe value of its feature id(s).
Evidence abbreviations: `VP` = `target-capability-verb-probes-20261105.txt` (the verb number in
that file), `MX` = `target-capability-matrix-20261105.json`, `C28`/`C30`/`C13`/`C17` etc. =
`docs/C-acceptance.md` sections, `re-*` = the sibling RE notes in this directory.

### 3.1 Identity, diagnostics, local executors

| command | axes | binding→live | dispatch | verdict | evidence |
|---|---|---|---|---|---|
| `info` | read | LocalOnly | Implemented | TI | exec VP-01 exit 0; platform `21VG`, BIOS `SWCN22WW`, 109 features |
| `doctor` | read | LocalOnly | Implemented | TI | exec VP-02: 15 checks, 3 honest failures (control.conflicts, keyboard.backlight, update.capability) |
| `cpu` (`CPU_STATUS`) | read | `cpu.status` (probe `record("cpu.status", …)`) | Implemented | NV | probe site exists; 14:23 build predates the row |
| `events` (`EVENTS_LOG`) | read | `events.log` | Implemented | IP + RL | no recording site anywhere (only the registry names it); `ServiceRole::Events` unmapped |
| `firmware` (`FIRMWARE_STATUS`) | read | `firmware.status` (probe `record("firmware.status", …)`) | Implemented | NV | probe site exists; not invocable by the 14:23 build |
| `scan list` | read | `diagnostics.inventory`→available | Implemented | TI | exec VP-32: `["battery","thermal","storage","memory","firmware","network"]` |
| `scan run` | read | `diagnostics.inventory`→available | Implemented | TI | exec VP-33: per-item measurements + outcomes, exit 0 |
| `wifi security` | read | `wifi.security`→available | Implemented | TI | exec VP-34: 4087 B of live interfaces/profiles/assessment |
| `fix rules` | read | LocalOnly | Implemented | TI | row is local; `fix` subcommand absent from the 14:23 build (EXIT=2), rule store proven by VP-36 |
| `fix scan` | read | `machinefix.scan`→available | Implemented | TI | exec VP-36: 2 rules evaluated with live service states |
| `fix run` | write | `machinefix.run`→available | Implemented | TI | probe `available`; verifier is per-rule readback (rule text in VP-36) |
| `fix track` | read | `machinefix.track`→available | Implemented | TI | probe `available`; applied-fix ledger |
| `scene list` | read | LocalOnly | Implemented | TI | exec VP-37: ruleset incl. GAMEZONE/`AI_SCENARIO` classes |
| `scene test` | write | `scene.test`→available | Implemented | TI | probe `available` (static registrar, `"scene.test"` tuple) |
| `scene track` | read | `scene.track`→available | Implemented | TI | probe `available` |
| `snapshot capture/diff/restore` | mixed | `Dynamic` legs | Implemented | LU | exec VP-40 `snapshot diff` exit 0; legs include `perf.fan.mode`/`tune.pl*`/`kbd.backlight` → `limited` |
| `daemon start/stop/status/install` | mixed | ServiceOnly | External | EX | exec VP-39: `channel_unavailable: vantage daemon IPC: 系统找不到指定的文件 (os error 2)`; bare `daemon` refuses with "handled by the platform binary" (VP-58) |
| `completions` | read | LocalOnly | Implemented | TI | exec VP-59 exit 0 |

### 3.2 Battery / USB / power

| command | axes | binding→live | dispatch | verdict | evidence |
|---|---|---|---|---|---|
| `battery status` | read | `battery.status`→available | Implemented | TI | exec VP-04/VP-47: real telemetry (design 99 900 mWh, full 93 060 mWh, chemistry Lion) |
| `battery adapter` | read | `battery.adapter`→available | Implemented | TI | exec VP-06: `ac_line_status: true`, `battery_flag: 1` |
| `battery health` | read | `battery.health` (probe `record("battery.health", …)`) | Implemented | TI | direct: `BatteryFullChargedCapacity=93060` + design 99900 from VP-04; subcommand absent from the 14:23 build (EXIT=2) |
| `battery charge-mode` | write | `battery.charge_mode`→available | Implemented | TI | C28: `normal` → GBMD `0x00860000`, restore → `0x00860004` (independent EnergyDrv readback) |
| `battery thresholds` | write | `battery.thresholds`→**unavailable** | Unsupported | TU | probe detail "target firmware has no verified arbitrary-threshold write channel"; no arbitrary-threshold IOCTL on `\\.\EnergyDrv` |
| `battery extreme-life` | write | `battery.extreme-life` (no probe) | Unsupported | TU | charge domain is exactly 0/1/2 over GBMD; no owned signed-kernel Job lifecycle (row's own reason) |
| `battery night-charge` | write | `battery.night-charge`→available | Implemented | TI | C28: `0x01`→`0x11`→`0x01` |
| `battery temporary-mode` | read | `battery.temporary-mode`→available | Implemented | TI | exec VP-07: `{"dynamic_lifespan":false,"permanent":true,"plug_in":false,"raw":1}` |
| `battery watch` | read | `battery.status`→available | Implemented | TI | same channel as VP-04; interval loop not run (would outlive the audit) |
| `usb always-on` (usb leg) | write | `usb.charging`→available | Implemented | TI | C28: flag `0xCEC0`→`0xCE40`→`0xCEC0` |
| `usb always-on` (bios leg) | write | `bios`→available (**flipped**) | Implemented | TI | exec VP-52 `bios get AlwaysOnUSB` = `Enable`; probe `for feature in ["bios", …]` |
| `usb charge-on-battery` (usb leg) | write | `usb.charging`→available | Implemented | TI | C28: `0xCEC0`→`0x4EC0`→`0xCEC0` |
| `usb charge-on-battery` (bios leg) | write | `bios`→available (**flipped**) | Implemented | TI | exec VP-54 `bios get ChargeInBatteryMode` = `Enable` |
| `power` / `power scheme` (`POWER_SCHEME_ACTIVE`) | read | `power.scheme`→available | Implemented | TI | exec VP-50/VP-51: active scheme `381b4222-…` 平衡 |
| `power scheme list` | read | `power.scheme`→available | Implemented | TI | exec VP-09 |
| `power scheme get <name>` | read | `power.scheme`→available | Implemented | TI | exec VP-51 |
| `power scheme get --subgroup/--setting` | read | `power.scheme.setting.read`→available | Implemented | TI | prior accepted read-only run `target-power-setting-read-20261029.json` |
| `power scheme apply` | write | `power.scheme`→available | Implemented | TG/LU | only mock isolation acceptance exists (C16); no physical apply acceptance recorded |
| `power scheme set` | write | `power.scheme.setting`→available | Implemented | TG/LU | read side accepted (above); write side not physically accepted |
| `power saver-once` | write | `power.saver-once`→available | Implemented | TG/LU | probe available; C17 notes full acceptance still outstanding |
| `power ac` (`POWER_AC`) | read | `battery.adapter`→available | Implemented | TI | exec VP-06 |
| `power draw` (`POWER_DRAW`) | read | `battery.status`→available | Implemented | TI | exec VP-04/VP-47 |

### 3.3 Performance / thermal / tuning

| command | axes | binding→live | dispatch | verdict | evidence |
|---|---|---|---|---|---|
| `perf mode` | write | `perf.mode`→**limited** | Implemented | TG | probe detail: vendor-only modes (geek/silent-HP/custom) have no OS equivalent; C23 native-overlay acceptance |
| `perf fan status` | read | `AnyOf(perf.fan.mode, perf.fan.inventory)` | Implemented | TI | exec VP-10: `fans:[{id:1,min:2100,max:5100}]`, `mode:{"unknown":4}`, `mode_error:null` |
| `perf fan auto/manual/fullspeed/smart` | write | `perf.fan.mode.write` | Implemented | TG | `re-fan-write-20261105.md` §1-§6: `GetSmartFanMode` → `WBEM_E_INVALID_OBJECT 0x8004100F`, `SetSmartFanMode` acknowledges once and never applies, `ITS_FanMode` is a write-only dispatcher projection → the CLI refuses before writing (`thermal.rs`: `"perf.fan.mode.write requires a firmware fan-mode readback, …"`) |
| `perf fan curve` | write | `perf.fan.curve` (no probe) | Unsupported | TU | `re-fan-write-20261105.md` §1: class declares 52 methods, no curve/`SetFanTale`; §4 `SetFanTale` has zero code references |
| `perf temp` (`list`) | read | `perf.temp`→available | Implemented | TI | exec VP-11/VP-41/VP-46: ACPI `TZ00_0` 46.05 °C; battery-class sensor honestly `unavailable` |
| `perf temp read <id>` | read | `perf.temp`→available | Implemented | TI | id must be the canonical `acpi/ACPI\ThermalZone\TZ00_0`; `id="acpi"` correctly rejected (VP-57, EXIT=2) |
| `perf temp watch` | read | `perf.temp`→available | Implemented | TI | same channel; interval loop not run |
| `perf pl1` / `perf pl2` | write | `tune.pl1`/`tune.pl2`→**unavailable** | Implemented | TG | probe: "Windows raw MSR/RAPL writes are unavailable"; direct: `Policy_DisablePLx=1`, `tune.msr` `permission denied` over this session; C28 records `unsupported: tune.pl1` |
| `perf dbdc get` | read | `perf.dbdc`→available | Implemented | TI | exec VP-12: `counts:3`, limits `[7500,5000,4500]` mA |
| `perf gpu status` | read | `perf.gpu`→unavailable | Implemented | TG | direct: `LENOVO_GAMEZONE_GPU_OC_DATA` class exists with **no instance**; `re-gpu-oc-20261105.md` §1: `IsSupportGpuOC OK Data=0`, `Policy_GPUOCStatus=0`, param block `0x8004100C` → verb reports `supported:false` |
| `perf policy` | read | `perf.policy` (probe `its_policy.rs`: `record("perf.policy", …)` from `read_dword("Policy_PLxSetMode")`) | Implemented | TI | direct: `Policy_PLxSetMode=0x00010001`, `Policy_Override=3`, `ITS_CurrentSetting=3`, `ITS_AutomaticModeSetting=7` in `…\LenovoProcessManagement\Performance\PowerSlider`; subcommand absent from the 14:23 build |
| `perf top` | read | `perf.top`→available | Implemented | TI | exec VP-15: process list with interval CPU sampling |
| `perf boost` / `perf throttle` | write | `perf.boost`/`perf.throttle`→available | Implemented | TI | probe: Win32 priority policy with independent readback |
| `tune profile list` | read | LocalOnly | Implemented | TI | exec VP-16: 5 profiles |
| `tune profile show <name>` | read | LocalOnly | Implemented | TI | exec VP-55: `balanced` decoded |
| `tune profile apply` | write | `Dynamic` (8 legs) | Implemented | LU | legs `perf.fan.mode`/`tune.pl*`/`kbd.backlight` gated → `limited`; `--dry-run` path exists (not run here) |
| `tune pl1/pl2/tau` (`TUNE_PL1`) | write | `Dynamic(tune.pl1,pl2,tau)`→unavailable | Implemented | TG | same MSR evidence as `perf pl1` |
| `tune epp` | write | `tune.epp`→available | Implemented | TI | probe detail "PERFEPP write+readback verified via PowerSetting mutation"; C5 real write |
| `tune turbo` | write | `tune.turbo`→available | Implemented | TI | `re`-probe detail "PERFBOOSTMODE write+readback verified"; C6 real write |
| `tune restore` | write | `Dynamic` (8 legs) | Implemented | LU | composite legs; restores only what it can verify (C30 pattern) |
| `tune telemetry` | read | `AnyOf(7 features)` | Implemented | TI | exec VP-17: live EPP/battery/perf/temp sample |
| `tune watch` | read | `AnyOf(4)` | Implemented | TI | same sources as VP-17; loop not run |

### 3.4 Keyboard, touchpad, panel

| command | axes | binding→live | dispatch | verdict | evidence |
|---|---|---|---|---|---|
| `kbd backlight` | write | `kbd.backlight` | Implemented | TI **+RL** | `re-kbd-backlight-20261105.md` §3: live `KBLC(0x1)` over `\\.\EnergyDrv` IOCTL `0x83102144` → `out=0x7` (tristate+auto, group 3, states 0..3); the 14:23 binary still reported the old `ROOT\WMI 0x8004100C` path. `ServiceRole::Keyboard` is unmapped → matrix would say `unverified` even now |
| `kbd fnlock` | write | `kbd.fnlock`→available | Implemented | TI | probe available; C30 accepts the staged write with `--auto-restore` |
| `kbd fn-ctrl-swap` | write | `kbd.fn-ctrl-swap`→available (**flipped**) | Implemented | TI | exec VP-53 `bios get FnAndCtrlKeySwap` = `Disable`; C30 accepts the staged write |
| `kbd winlock` | write | `wmi.gamezone.methods`→**unavailable** | Unsupported | TU | probe: "21VG firmware returns Invalid object for the GAMEZONE method family"; `re-fan-write` §1 `0x8004100F` for the whole family |
| `touchpad on/off` (`TOUCHPAD`) | write | `touchpad`→available | Implemented | TI | probe available; C7 recovery/disable defect fixed and accepted |
| `touchpad status` | read | `touchpad.status`→available (**flipped**) | Implemented | TI | exec VP-38: `{"actual":"enabled"}` |
| `touchpad recover` | write | `touchpad.recover`→available (**flipped**) | Implemented | TI | probe `available` (`record("touchpad.recover", …)`, from live leaf enumeration); dry-run accepted in C13; `re-touchpad-20261105.md` §2 |
| `panel rate` | write | `panel.refresh`→available | Implemented | TI | C1: verified 60/120 Hz transitions on the internal panel (interactive session) |
| `panel info` (`PANEL_INFO`) | read | `panel.identity` (probe `record("panel.identity", …)`) | Implemented | NV | probe site exists; subcommand absent from the 14:23 build |
| `panel brightness` (status) | read | `panel.brightness.status`→available (**flipped**) | Implemented | TI | exec VP-18: `current_percent:100`, levels 0..100 |
| `panel brightness <pct>` | write | `panel.brightness.set`→available (**flipped**) | Implemented | TI | probe from `WmiMonitorBrightnessMethods` (`record("panel.brightness.set", …)`); C17 restricted write acceptance |
| `panel color` | write | `panel.color.write` (no probe) | Unsupported | TU | `re-panel-20261105.md` §1: gamut switching needs `MONITORCONTROL.dll` (not shipped on this SKU) |
| `panel super-resolution` | write | `panel.super-resolution.write` (no probe) | Unsupported | TU | `re-panel` §4: driver rejects the SR state query (`0x8004100C`), so no write contract |
| `panel overdrive` | write | `panel.overdrive.write` (no probe) | Unsupported | TU | `re-panel` §2/§3: no SPBTESTTOOL PnP device, `BiosOD` capability bits absent |
| `panel eye-care` | write | `panel.eye-care.write` (no probe) | Unsupported | TU | `re-panel` §5: Intel pixel-transformation block not honoured by this driver |

### 3.5 Privacy, sensing, audio

| command | axes | binding→live | dispatch | verdict | evidence |
|---|---|---|---|---|---|
| `privacy cam` (runtime) | write | `privacy.camera.runtime`→available | Implemented | TI | probe available (`channel.privacy-runtime`); C30 covers the persistent leg |
| `privacy cam --persistent` | write | `privacy.camera`→available | Implemented | TI | C30: staged BIOS write + `--auto-restore` readback; `re-privacy-cam-impl-20261105.md` |
| `privacy mic` (runtime) | write | `privacy.microphone.runtime`→available | Implemented | TI | probe available; `re-privacy-cam-impl` |
| `privacy mic --persistent` | write | `privacy.microphone`→available | Implemented | TI | C30 staged write |
| `privacy fingerprint` | write | `privacy.fingerprint`→available | Implemented | TI | C30 staged write; `re-privacy-cam-impl` |
| `privacy status` | read | `AnyOf(camera,microphone,fingerprint)` | Implemented | TI | exec VP-19: BIOS values + `runtime_camera/microphone` |
| `privacy cam status` | read | same | Implemented | TI | subcommand absent from the 14:23 build (EXIT=2); same channel as VP-19 |
| `sense status` | read | `sense.status`→available (**flipped**) | Implemented | TI | exec VP-21: `source:"hpd-rpc"`, `advertised:2047`, all eight settings decoded |
| `sense lock-on-leave` | write | `sense.lock-on-leave`→available (**flipped**) | Implemented | TI-read / TG-write | probe derives availability from the HPD **advertised** bitmask (`("sense.lock-on-leave", bit(0x0010))`); the write itself has no accepted run (see §6.5) |
| `sense wake-on-approach` | write | `sense.wake-on-approach`→available (**flipped**) | Implemented | TI-read / TG-write | `("sense.wake-on-approach", bit(0x0002))`; same caveat |
| `sense pause-video` | write | `sense.pause-video`→available (**flipped**) | Implemented | TI-read / TG-write | `("sense.pause-video", bit(0x0200))`; `sense-parity-acceptance-20261102.md`: legacy pipe rejected `0x000d0101 arg 2` |
| `sense camera-assist` | write | `sense.camera-assist`→available (**flipped**) | Implemented | TI-read / TG-write | `("sense.camera-assist", bit(0x0040))` |
| `sense approach-auto-adjust` | write | `sense.approach-auto-adjust`→available (**flipped**) | Implemented | TI-read / TG-write | `("sense.approach-auto-adjust", bit(0x0008))` |
| `sense leave-auto-adjust` | write | `sense.leave-auto-adjust`→available (**flipped**) | Implemented | TI-read / TG-write | `("sense.leave-auto-adjust", bit(0x0100))` |
| `sense attention-tracking` | write | `sense.attention-tracking`→available (**flipped**) | Implemented | TI-read / TG-write | `("sense.attention-tracking", true)` |
| `sense reset` | write | `sense.reset`→available (**flipped**) | Implemented | TI-read / TG-write | `("sense.reset", true)` (op 7 advertised) |
| `sense kbd-light-auto` | write | `sense.kbd-light-auto` (no probe) | Unsupported | TU | no consuming service on this model (row reason); `re-identity-20261105.md` |
| `audio dolby` | write | `audio.dolby`→available (**flipped**) | Implemented | TI | C28: write accepted, vendor readback `GetDolbyEnabled=True/GetActiveProfile=0`, restored to `False/5`; `re-audio-write-20261105.md` §1 |
| `audio noise-cancel` | write | `audio.noise-cancel`→available (**flipped**) | Implemented | TI | C28: `GetCurrentMode 0→1→0` |
| `audio endpoints` | read | `audio.endpoints` (probe `for feature in ["audio.endpoints", "audio.volume", "audio.mute"]`) | Implemented | NV | probe site exists; subcommand absent from the 14:23 build |
| `audio volume` | write | `audio.volume` (same registrar array) | Implemented | NV | same |
| `audio mute` | write | `audio.mute` (same registrar array) | Implemented | NV | same |

### 3.6 BIOS

| command | axes | binding→live | dispatch | verdict | evidence |
|---|---|---|---|---|---|
| `bios list` | read | `bios`→available (**flipped**) | Implemented | TI | exec VP-22: 34 `Lenovo_BiosSetting` rows with selections + values |
| `bios get <name>` | read | `bios`→available (**flipped**) | Implemented | TI | exec VP-52/53/54/56 |
| `bios set <name> <value>` | write | `bios`→available (**flipped**) | Implemented | TI | probe `bios.staging.contract` available ("staging-buffer writes contract-verified"); C30 accepted |
| `bios save` | write | `bios.save.global-buffer` (no probe) | Unsupported | TU | global single staging buffer is machine-wide; standalone save would commit other clients' staged values (C30 finding) |
| `bios discard` | write | `bios.discard`→available (**flipped**) | Implemented | TI | same `for feature in ["bios", …]` probe as `bios`; C30 |
| `bios defaults` | write | `bios.defaults`→available (**flipped**) | Implemented | TI | probe: `Lenovo_LoadDefaultSettings` has an active instance (`("bios.defaults", "Lenovo_LoadDefaultSettings")`); not executed here |
| `bios password status` | read | `bios.password.status`→available (**flipped**) | Implemented | TI | exec VP-23: `Lenovo_BiosPasswordSettings` read |
| `bios password set` | write | `bios.password.set` (no probe) | Unsupported | TU | documented no-password-write boundary |
| `bios password clear` | write | `bios.password.clear` (no probe) | Unsupported | TU | same boundary |
| `bios password verify` | read | `bios.password.verify`→available (**flipped**) | Implemented | TI | probe: `Lenovo_SetBiosPassword` active instance (`("bios.password.verify", "Lenovo_SetBiosPassword")`); not executed here |

### 3.7 MagicBay, OSD, hotkeys

| command | axes | binding→live | dispatch | verdict | evidence |
|---|---|---|---|---|---|
| `magicbay detect` | read | `magicbay.inventory`→limited | Implemented | TI | exec VP-24: `{"acpi_devices":[],"devices":[]}` — no accessory attached; probe `limited` (SetupAPI via the MagicBay service) |
| `magicbay lte status` | read | `magicbay.lte` | Implemented | TG | exec VP-25: `unsupported: standard MBIM/UVC/DRM control is not attached`; direct: `root\wmi:MbnInterface` → 无效类 (class absent) |
| `magicbay lte connect/disconnect/apn` | write | `magicbay.lte` | Unsupported | TU | same absent MBIM plane; `re-magicbay-20261105.md` §3 |
| `magicbay cam` | read | `magicbay.cam` (no probe) | Unsupported | TU | exec VP-26 unsupported; no UVC control plane |
| `magicbay display` | read | `magicbay.display` (no probe) | Unsupported | TU | exec VP-27 unsupported; no display control plane |
| `magicbay watch` | read | ServiceOnly | Unsupported | TU | no documented MagicBay event class (row reason) |
| `osd enable` / `osd disable` | write | `AnyOf(osd.fn.capslock, osd.fn.numlock)` | Implemented | TI | `re-osd-20261105.md` §2: pinned target `LenovoFnAndFunctionKeys` controls 146/147, 148/149 with registry readback; `record_fn_osd` reads the Fn-package registry and records `osd.fn.capslock`/`osd.fn.numlock`; not executed here |
| `osd test` / `osd show` | mixed | `osd.test`/`osd.show`→available (**flipped**) | Implemented | TI | static registrar `Availability::Available` (`"osd.test"`/`"osd.show"` tuples) with "session-gated at show time"; matrix MX reports `available` |
| `hotkey list` | read | `hotkey.list`→available (**flipped**) | Implemented | TI | exec VP-28: `{"registered":[]}` (nothing registered in this session) |
| `hotkey status` | read | `AnyOf(channel.hotkey, kbd.fnlock)` | Implemented | TI | `re-hotkeys-20261105.md` §1-§3: live SCM/registry reads of `LenovoFnAndFunctionKeys` (Running/Automatic), renderers, `Capbility` flags; subcommand absent from the 14:23 build |
| `hotkey register` / `unregister` | write | `hotkey.register/unregister`→available (**flipped**) | Implemented | TI | probe `available` ("RegisterHotKey is session-gated at command time"); not executed here |
| `hotkey trigger` | read | `hotkey.trigger`→available (**flipped**) | Implemented | TI | probe `available` (in-process dispatch) |

### 3.8 Update, radio, security, storage

| command | axes | binding→live | dispatch | verdict | evidence |
|---|---|---|---|---|---|
| `update check` | read | `update.capability` (no probe) | **Unsupported** | **SU** | exec VP-30: exit 0, `last_sync 2026-10-05T20:02:25+08:00`, 3 packages with hashes/URLs. The row's reason only covers the *remote* leg — see §4 |
| `update local-driver apply` (`UPDATE_LOCAL_INSTALL`) / `update local-driver rollback` (`UPDATE_LOCAL_ROLLBACK`) | write | `update.local_driver(.rollback)`→available | Implemented | TI | probe `available`; WinTrust-verified SetupAPI path (distinct from `update local-install`, which installs a UDC package's own `install.ps1`) |
| `update local-install <dir>` | write | `update.local_package` (no probe) | Implemented | IP | gates implemented (`re-update-install-20261105.md` §4.2); never executed end-to-end (§5 of that note); no probe id exists |
| `update local history/ignore/schedule` | mixed | `update.local_driver.policy`→available | Implemented | TI | probe `available` ("local persisted update-policy store") |
| `update history` | read | `update.history.ledger` (no probe) | Implemented | IP | exec VP-31 `unsupported` **only because the binary predates the implementation**; direct: both ledgers exist (`…\session\editable_update_history.db`, `update_history.db`, 12 288 B each); `re-update-install` §2/§4.1 read 4 + 30 rows live. No probe id → matrix can only say `unverified` |
| `update download/install/ignore/rollback/schedule` | write | `update.*.without-…` (no probe) | Unsupported | TU | UDC mTLS is service-held (`udc-mtls-credential-hunt-20261105.md`); `api.cnno1.uds.lenovo.com` → 401 without the PCM credential; no install ledger binds prior versions |
| `radio status/off/on` | mixed | `radio.status/off/on` | Implemented | IP + RL | no recording site anywhere; live plane exists (direct: `MSFT_NetAdapter` shows `Intel Wi-Fi 7 BE213` WLAN + Bluetooth PAN; `MbnInterface` class absent → no WWAN); `ServiceRole::Radio` unmapped |
| `security status` | read | `security.status` | Implemented | IP + RL | no recording site; `ServiceRole::Security` unmapped |
| `storage status` | read | `storage.status` | Implemented | IP + RL | no recording site; `ServiceRole::Storage` unmapped |

---

## 4. Payload A — `DispatchSupport::Unsupported` whose channel is live

Exactly one row still refuses while the machine serves the verb.

### A1. `UPDATE_CHECK` — `update check` (stale-Unsupported)

* Registry: `FeatureBinding::Unprobed("update.capability")`,
  `DispatchSupport::Unsupported("remote update checking has no authenticated catalog contract")`.
* Live: `vantage --json update check` → **exit 0**, real payload (VP-30):
  `last_sync: 2026-10-05T20:02:25+08:00`, three packages (`Lenovo MagiCenter 2.0.5.501`,
  `Lenovo Smart Meeting …`, plus one more) each with `download_url`, `file_hash`, `size_bytes`,
  `update_type`.
* Why both facts are true: the dispatch (`crates/vantage-cli/src/lib.rs`, the
  `UpdateCommand::Check` arm) *prefers* a synced-catalogue report
  (`UpdateControl::check_updates()`) and falls back to the capability
  summary. What is unsupported is the **remote** refresh (UDC mTLS, 401 without the Lenovo-held
  client credential), which is exactly what `vantage doctor` reports for `update.capability`.
* Effect on users: `vantage capability` (and the docs matrix) tells them a working read command is
  `unsupported`. The row is selected by the `update check` verb, so the reason text is attached to
  the wrong layer.
* Not fixed here: the honest shape needs a *new* capability id for the synced catalogue (e.g.
  `update.catalog.local`) recorded by `windows_hal` when `LastUpdateConfig` parses — a probe/surface
  owned by another worker. Options for the parent: (a) new probe id + `Capability(...)`, or (b) keep
  `Unsupported` but re-word it and add a separate `IMPLEMENTED` note, or (c) split the row into
  remote-refresh vs local-catalogue. **Decision required; nothing was changed.**

Sanity check that this is a real drift and not one-off: during this audit the sibling working on
update flipped `UPDATE_HISTORY` from `Unsupported` to `Implemented` (it was `Unsupported` in the
14:23 build's registry, `Implemented` with the ledger note in today's tree) — i.e. this family is
being brought up row-by-row and `UPDATE_CHECK` is the row left behind.

---

## 5. Payload B — `Implemented` rows whose channel is dead or unprobed

### 5.1 Implemented, channel genuinely dead on this SKU (deliberate, cross-platform)

| row | live reason |
|---|---|
| `PERF_PL1`, `PERF_PL2`, `TUNE_PL1` | `tune.pl1/pl2/tau` recorded `unavailable`; `tune.msr` denied in this session (`\\.\WinMsrDev` ERROR_ACCESS_DENIED for a network logon); direct `Policy_DisablePLx=1` (the dispatcher owns PLx). The row is `Implemented` because the Linux backend uses `powercap`. |
| `PERF_FAN_AUTO/MANUAL/FULLSPEED/SMART` | `perf.fan.mode.write` recorded `unavailable`; `re-fan-write-20261105.md` proves there is no verifiable write on this firmware. Runtime refuses before writing. |
| `MAGICBAY_LTE_STATUS` | `magicbay.lte` unavailable; MBIM plane absent (`MbnInterface` class missing). |

These are **not** lying rows: the feature probe resolves them to `unsupported`/`unavailable` with a
reason, and the verbs refuse. Only the *static* column reads `Implemented`, which the `DispatchSupport`
doc comment defines as "the current CLI dispatch reaches this service/executor. It is not a
hardware-effect claim".

### 5.2 Implemented, but no probe records the bound feature id (matrix is blind)

`vantage capability` maps an absent feature id to `Unverified`, so these rows can never be reported
`available` even when they work:

| row | bound id | live fact that the channel is real |
|---|---|---|
| `UPDATE_LOCAL_PACKAGE` | `update.local_package` | gates + script contract implemented (`re-update-install` §4.2), unexecuted end-to-end |
| `UPDATE_HISTORY` | `update.history.ledger` | both ledgers present on disk; sibling read 4 + 30 rows live |
| `RADIO_STATUS/OFF/ON` | `radio.status/off/on` | WLAN + Bluetooth adapters present (direct CIM); no recorded probe |
| `EVENTS_LOG` | `events.log` | no probe; `ServiceRole::Events` unmapped |
| `STORAGE_STATUS` | `storage.status` | no probe; role unmapped |
| `USB_PORTS` (§11) | `usb.ports` | no probe; bound as `Capability(...)` to an id nothing records |
| `WIFI_STATUS` (§11) | `wifi.status` | no probe |
| `BLUETOOTH_LIST` (§11) | `bluetooth.list` | no probe; `ServiceRole::Bluetooth` unmapped |

Recommended owner action (not taken): add one `capabilities.record(...)` per id in
`windows_hal::capabilities()` with the real precondition, then flip the binding to `Capability(id)`.

`SECURITY_STATUS` was blind at the snapshot and stopped being blind mid-audit: a sibling added
`for channel in ["channel.security", "security.status"]` in `windows_hal.rs`. Its role
(`Security`) is still unmapped, so it will keep reporting `unverified` until that is fixed.

### 5.3 New rows whose probe exists but this audit could not observe it

`CPU_STATUS` (`record("cpu.status", …)`), `FIRMWARE_STATUS` (`record("firmware.status", …)`),
`PANEL_INFO` (`record("panel.identity", …)`), `AUDIO_ENDPOINTS`, `AUDIO_VOLUME`, `AUDIO_MUTE`
(`for feature in ["audio.endpoints", "audio.volume", "audio.mute"]`), `HOTKEY_STATUS`
(`channel.hotkey`/`kbd.fnlock`). Their bindings are already `Capability(...)`/`AnyOf(...)`, so there
is nothing to flip; they need a rebuild to be observed (§10).

---

## 6. Matrix-precision defects found (reported, not fixed)

### 6.1 `UPDATE_CHECK` — see §4.

### 6.2 `ServiceRole` values with no `role_channel()` entry

`crates/vantage-cli/src/capability.rs::role_channel` maps 26 roles; the enum has 35. Unmapped:
`Bluetooth`, `Events`, `Keyboard`, `MagicBayCamera`, `MagicBayDisplay`, `Radio`, `Security`,
`Storage`.
`role_present()` returns `Unexposed` for them, and `evaluate_command` turns that into
`ObservedStatus::Unverified` — so a row bound to one of these roles can *never* be `available`,
whatever the probe says. Affected rows (dispatch `Implemented`): `KBD_BACKLIGHT`, `EVENTS_LOG`,
`RADIO_STATUS`, `RADIO_OFF`, `RADIO_ON`, `SECURITY_STATUS`, `STORAGE_STATUS`, `BLUETOOTH_LIST`,
plus `TUNE_PROFILE_APPLY`/`TUNE_RESTORE` (Keyboard inside a `Composite`). Today the worst case is
`kbd backlight`: the channel is live (`KBLC` over EnergyDrv) yet the matrix will say `unverified`.
Fix belongs to whoever records the corresponding `channel.<role>` ids — not done here because it
changes a shared evaluator and needs new probe ids.

### 6.3 Selector over-permissiveness

`command_selector`/`command_id` map bare `vantage power` to `POWER_SCHEME_ACTIVE`, but bare `power`
is not a read form — the binary refuses it (`EXIT=3`, "a power subcommand is required", VP-08);
`power scheme` is the read form and works (VP-50). Cosmetic mapping nit: a user asking for
"power" gets the power-scheme row's verdict.

### 6.4 `update check` doctor/matrix disagreement

`vantage doctor` reports `update.capability` as FAIL ("no authenticated public Lenovo update
catalog/manifest contract is specified") while `update check` returns a real catalogue. Both are
honest about different legs; the two surfaces should name them differently (see §4).

### 6.5 Advertisement is not acceptance (sense writes)

The nine `sense.*` ids are computed from the HPD **advertised** bitmask
(`windows_hal.rs`: `("sense.lock-on-leave", bit(0x0010))`, `("sense.wake-on-approach", bit(0x0002))`, …).
`available` therefore means
"the firmware advertises the verb", not "a write was accepted". The only recorded write attempt is
`sense-parity-acceptance-20261102.md`: the legacy SmartSense pipe rejected
`0x000d0101 arg 2`, registry stayed `Video=1`. The HPD-RPC write path is newer and has no executed
acceptance in the evidence set. Recommendation (owner decision): record a distinct
`<id>.write`/`.advertised` key, or state the semantics in the row note, so that `Capability(...)`
on a mutation row does not over-claim.

### 6.6 Stale binary ⇒ stale matrix

The 14:23 build's matrix has 134 rows; the tree has 153. Any published capability matrix must be
regenerated from a build of the audited tree (§10). Concrete mismatches observed: `perf.fan.*`
`feature_ids` (`perf.fan.mode` vs `perf.fan.mode.write`), `kbd.backlight` (`ROOT\WMI 0x8004100C` vs
the live `KBLC` channel), `update history` (`unsupported` vs `implemented`), and 13 rows that the
binary cannot parse.

---

## 7. Applied change — 37 `Unprobed → Capability` flips

Rule applied (per parent instruction): flip only where **both** hold, and keep the id string
identical:

1. a recording site exists in `crates/lctrl-hal-win/src/windows_hal.rs` (quoted below), and
2. the live capability run reports that id `available`/`limited`, i.e. the probe proved the channel;
3. the row's dispatch is `Implemented`.

No behavioural change: `feature_presence()` consults the `CapabilitySet` for `Unprobed` and
`Capability` through the same `capability_presence()` call
(`crates/vantage-cli/src/capability.rs`, `feature_presence()`:
`Unprobed(feature) | Capability(feature) => capability_presence(feature, capabilities)`); the flip
only stops the registry claiming "no current capability probe" when one exists.

Recording sites are cited by their enclosing statement/array, not by line number: `windows_hal.rs`
is being extended by sibling workers continuously (it grew by well over a hundred lines during this
audit alone), so any line number would be stale within minutes. Every anchor below was re-checked
against the tree when this table was written; `git grep -n '<anchor>' crates/lctrl-hal-win/src`
locates it.

| # | row | id | recording site in `windows_hal.rs` | live observation |
|---:|---|---|---|---|
| 1 | `USB_ALWAYS_ON` (bios leg) | `bios` | `for feature in ["bios", "bios.discard", "kbd.fnlock", "kbd.fn-ctrl-swap"]` | MX `usb.always-on` (bios leg) `available`; exec VP-52 `bios get AlwaysOnUSB` = `Enable` |
| 2 | `USB_CHARGE_ON_BATTERY` (bios leg) | `bios` | same array as row 1 | MX `available`; exec VP-54 `ChargeInBatteryMode` = `Enable` |
| 3 | `KBD_FN_CTRL_SWAP` | `kbd.fn-ctrl-swap` | same array as row 1 | MX `available`; exec VP-53 `FnAndCtrlKeySwap` = `Disable`; C30 write acceptance |
| 4 | `TOUCHPAD_STATUS` | `touchpad.status` | `for feature in ["channel.touchpad", "touchpad", "touchpad.status"]` | MX `available`; exec VP-38 `{"actual":"enabled"}` |
| 5 | `TOUCHPAD_RECOVER` | `touchpad.recover` | `record("touchpad.recover", …)` | MX `available`; C13 dry-run; `re-touchpad` §2 |
| 6 | `PANEL_BRIGHTNESS_STATUS` | `panel.brightness.status` | `record("panel.brightness.status", …)` | MX `available`; exec VP-18 `current_percent:100` |
| 7 | `PANEL_BRIGHTNESS_SET` | `panel.brightness.set` | `record("panel.brightness.set", …)` (`WmiMonitorBrightnessMethods` precondition) | MX `available`; C17 write acceptance |
| 8 | `SENSE_STATUS` | `sense.status` | `("sense.status", true)` | MX `available`; exec VP-21 (`source:"hpd-rpc"`) |
| 9 | `SENSE_LOCK_ON_LEAVE` | `sense.lock-on-leave` | `("sense.lock-on-leave", bit(0x0010))` | MX `available`; VP-21 `lock_on_leave:true` |
| 10 | `SENSE_WAKE_ON_APPROACH` | `sense.wake-on-approach` | `("sense.wake-on-approach", bit(0x0002))` | MX `available`; VP-21 `wake_on_approach:true` |
| 11 | `SENSE_PAUSE_VIDEO` | `sense.pause-video` | `("sense.pause-video", bit(0x0200))` | MX `available`; VP-21 exposes the field |
| 12 | `SENSE_CAMERA_ASSIST` | `sense.camera-assist` | `("sense.camera-assist", bit(0x0040))` | MX `available`; VP-21 `camera_assist:true` |
| 13 | `SENSE_APPROACH_AUTO_ADJUST` | `sense.approach-auto-adjust` | `("sense.approach-auto-adjust", bit(0x0008))` | MX `available`; VP-21 `approach_auto_adjust:true` |
| 14 | `SENSE_LEAVE_AUTO_ADJUST` | `sense.leave-auto-adjust` | `("sense.leave-auto-adjust", bit(0x0100))` | MX `available`; VP-21 `leave_auto_adjust:true` |
| 15 | `SENSE_RESET` | `sense.reset` | `("sense.reset", true)` | MX `available` |
| 16 | `SENSE_ATTENTION_TRACKING` | `sense.attention-tracking` | `("sense.attention-tracking", true)` | MX `available` |
| 17 | `AUDIO_DOLBY` | `audio.dolby` | `record("audio.dolby", …)` | MX `available`; C28 write + vendor readback |
| 18 | `AUDIO_NOISE_CANCEL` | `audio.noise-cancel` | `record("audio.noise-cancel", …)` | MX `available`; C28 write + SDK readback |
| 19 | `BIOS_LIST` | `bios` | same array as row 1 | MX `available`; exec VP-22 (34 settings) |
| 20 | `BIOS_GET` | `bios` | same array as row 1 | MX `available`; exec VP-52/53/54/56 |
| 21 | `BIOS_SET` | `bios` | same array as row 1 (+ `bios.staging.contract`) | MX `available`; C30 accepted staged write |
| 22 | `BIOS_DISCARD` | `bios.discard` | same array as row 1 | MX `available`; C30 |
| 23 | `BIOS_DEFAULTS` | `bios.defaults` | `("bios.defaults", "Lenovo_LoadDefaultSettings")` | MX `available` |
| 24 | `BIOS_PASSWORD_STATUS` | `bios.password.status` | `record("bios.password.status", …)` | MX `available`; exec VP-23 |
| 25 | `BIOS_PASSWORD_VERIFY` | `bios.password.verify` | `("bios.password.verify", "Lenovo_SetBiosPassword")` | MX `available` |
| 26 | `OSD_TEST` | `osd.test` | `"osd.test"` tuple in the OSD registrar | MX `available`; `re-osd` |
| 27 | `OSD_SHOW` | `osd.show` | `"osd.show"` tuple in the OSD registrar | MX `available`; `re-osd` |
| 28 | `HOTKEY_LIST` | `hotkey.list` | `"hotkey.list"` tuple in the hotkey registrar | MX `available`; exec VP-28 |
| 29 | `HOTKEY_REGISTER` | `hotkey.register` | `"hotkey.register"` in the same tuple | MX `available`; `re-hotkeys` |
| 30 | `HOTKEY_TRIGGER` | `hotkey.trigger` | `"hotkey.trigger"` in the same tuple | MX `available` |
| 31 | `HOTKEY_UNREGISTER` | `hotkey.unregister` | `"hotkey.unregister"` in the same tuple | MX `available` |
| 32 | `WIFI_SECURITY` | `wifi.security` | `record("wifi.security", …)` | MX `available`; exec VP-34 |
| 33 | `FIX_SCAN` | `machinefix.scan` | `"machinefix.scan"` tuple in the MachineFix registrar | MX `available`; exec VP-36 |
| 34 | `FIX_RUN` | `machinefix.run` | `"machinefix.run"` in the same tuple | MX `available`; rule readback described in VP-36 |
| 35 | `FIX_TRACK` | `machinefix.track` | `"machinefix.track"` in the same tuple | MX `available` |
| 36 | `SCENE_TEST` | `scene.test` | `"scene.test"` tuple in the scene registrar | MX `available`; exec VP-37 ruleset |
| 37 | `SCENE_TRACK` | `scene.track` | `"scene.track"` in the same tuple | MX `available` |

This change is exactly 37 lines, each `FeatureBinding::Unprobed("X"),` →
`FeatureBinding::Capability("X"),`, applied as one `apply_patch` to the live file. Nothing else in
the file was touched by this task; the file also carries unrelated sibling work (see §1).

---

## 8. Left `Unprobed`, with the reason

### 8.1 No recording site exists for the id (19 in the snapshot, 21 counting §11)

These stay `Unprobed` — flipping them would claim a probe that nothing writes.

| rows | ids |
|---|---|
| `PANEL_COLOR`, `PANEL_OVERDRIVE` | `panel.color.write`, `panel.overdrive.write` |
| `SENSE_KBD_LIGHT_AUTO` | `sense.kbd-light-auto` |
| `BIOS_SAVE`, `BIOS_PASSWORD_SET`, `BIOS_PASSWORD_CLEAR` | `bios.save.global-buffer`, `bios.password.set`, `bios.password.clear` |
| `MAGICBAY_CAM`, `MAGICBAY_DISPLAY` | `magicbay.cam`, `magicbay.display` |
| `UPDATE_CHECK`, `UPDATE_LOCAL_PACKAGE`, `UPDATE_DOWNLOAD`, `UPDATE_INSTALL`, `UPDATE_HISTORY`, `UPDATE_IGNORE`, `UPDATE_ROLLBACK`, `UPDATE_SCHEDULE` | `update.capability`, `update.local_package`, `update.download.without-authenticated-manifest`, `update.install.without-authenticated-manifest`, `update.history.ledger`, `update.ignore.without-catalog`, `update.rollback.without-install-ledger`, `update.schedule.without-catalog` |
| `RADIO_STATUS`, `RADIO_OFF`, `RADIO_ON` | `radio.status`, `radio.off`, `radio.on` |
| `WIFI_STATUS`, `BLUETOOTH_LIST` (§11) | `wifi.status`, `bluetooth.list` |

Note the eight `update.*` ids here are **deliberately** non-probe names ("without-auth…",
"global-buffer") — they describe a refused contract, not a channel. Only
`update.capability`/`update.local_package`/`update.history.ledger` are candidates for a real probe
(§4, §5.2).

### 8.2 Recording site exists, but the live proof is insufficient (7)

| rows | id | recording site | why not flipped |
|---|---|---|---|
| `BATTERY_THRESHOLDS` | `battery.thresholds` | `record("battery.thresholds", …)` | probe records it **`unavailable`** ("target firmware has no verified arbitrary-threshold write channel"); the channel is not proven usable, and the row is `Unsupported` |
| `MAGICBAY_LTE_CONNECT`, `…_DISCONNECT`, `…_APN` | `magicbay.lte` | `for channel in ["channel.mobilebroadband", "magicbay.lte"]` | probe cannot reach it (MBIM plane absent; `magicbay lte status` → unsupported), so the id is `unavailable`; rows are `Unsupported` |
| `AUDIO_ENDPOINTS`, `AUDIO_VOLUME`, `AUDIO_MUTE` | `audio.endpoints`, `audio.volume`, `audio.mute` | `for feature in ["audio.endpoints", "audio.volume", "audio.mute"]` | recording site exists but the 14:23 build predates it, so this audit has **no live observation** for these ids; flip after the next build if MX shows them `available` |

---

## 9. What this audit did not run (honest limits)

* **No mutation of any kind was executed** — no `--yes`, no plain write, not even a `--dry-run`
  write path. Write rows are classified from the capability probe + the repo's recorded acceptance
  (`C28`, `C30`, `C1`, `C5`, `C6`, `C13`, `C17`) or flagged as unaccepted.
* The `battery health`, `perf policy`, `perf gpu`, `privacy cam status`, `hotkey status`,
  `fix rules`, `panel info`, `cpu`, `events`, `security`, `storage`, `radio`, `audio
  volume|mute|endpoints` verbs could not be invoked (binary predates them). Their verdicts rest on
  the recording site + probe, and are marked `NV`/`IP` instead of `TI`.
* `perf temp watch`, `tune watch`, `battery watch`, `magicbay watch` loops were not run (they do not
  terminate inside a bounded audit); their sources are the same verified sample paths.
* `PERF_FAN_STATUS`'s `mode` reads `{"unknown":4}` — that is the `ITS_FanMode=4` dispatcher
  projection, deliberately *not* named. It is not evidence of a fan mode.
* The `update check` finding (§4) is a *dispatch* observation on the 14:23 binary whose
  `update check` implementation is present in the current tree (`lib.rs`, `UpdateCommand::Check`
  arm); the parent's rebuild
  must re-confirm exit 0.

---

## 10. Proposed parent acceptance (and negative controls)

1. **Rebuild and re-run the matrix** on the target:
   `cargo.exe build --offline --manifest-path vantage\Cargo.toml --bin vantage`, then
   `vantage.exe --json capability > cap-after.json`.
   *Expect*: every row that exists in both runs must be **byte-identical** to
     `target-capability-matrix-20261105.json` — `status`, `detail`, `feature_ids`,
     `service_roles`, `dependencies`, `axes`. The flip cannot alter output at all: `status` comes
     from `capability_presence()` for both variants, and `feature_ids` come from
     `crates/vantage-cli/src/capability.rs` (`Capability(feature) | Unprobed(feature) =>
     vec![feature]`), which also prints both identically. Any delta is a real regression from
     concurrent edits, not from the flip.
2. **Negative control for the flip claim**: temporarily patch one flipped row back to `Unprobed`,
   rebuild, and re-run `vantage capability`; the JSON must be unchanged. (Cheap, conclusive.)
3. **`update check` positive**: on this target, `vantage --json update check` → exit 0 with
   `last_sync`/`packages` (§4). **Negative**: on a machine without `…\Udc\LastUpdateConfig` (or on
   Linux), the same command must fall back to the capability summary and must not fabricate a
   catalogue.
4. **`kbd backlight` role defect (§6.2)**: after a build with the `KBLC` channel, `vantage capability`
   should report `kbd.backlight` `available`; with `ServiceRole::Keyboard` still unmapped it will
   say `unverified` — this confirms the defect rather than the channel.
5. **Registry tests**: `cargo test -p lctrl-hal` must still pass. Verified by grep at audit time:
   no crate test outside `command_capability.rs` mentions `Unprobed` at all
   (`crates/vantage-cli/src/capability.rs:1081` and `:1178` are the only other occurrences, both
   match arms), so no test can distinguish the two bindings.
6. **Diff control**: the tree is shared and dirty by design, so a plain `git diff` on the registry
   also contains sibling work. The check is per id against §7: each of the 37 listed ids must now
   read `FeatureBinding::Capability("<id>")`, and no id outside that table may have been flipped by
   *this* change (peers own their own flips: `osd.enable`/`osd.disable`, `magicbay.lte`,
   `update.history.without-catalog` landed during the same window). No dispatch, axes, dependency
   or `notes` line was touched.

---

## 11. Addendum — rows that landed after the audited snapshot

Four `CommandCapability` entries were added by sibling workers while this document was being
written (153 → 157 rows). They are classified here with the same rules; none of them is flipped,
because none has both a recording site and a live observation from the 14:23 binary.

| command | axes | binding | dispatch | verdict | why |
|---|---|---|---|---|---|
| `USB_PORTS` | read | `Capability("usb.ports")` | Implemented | **IP** | no recording site for `usb.ports` anywhere: a `Capability` binding to an id nothing records is the blind case of §5.2 (it reads as `unverified`, never `available`) |
| `TOUCHPAD_CAPS` | read | `Capability("touchpad.caps")` | Implemented | NV | probe added with the row (`record("touchpad.caps", …)`); the 14:23 build predates it, so no live value yet |
| `WIFI_STATUS` | read | `Unprobed("wifi.status")` | Implemented | **IP** | no recording site; stays `Unprobed` (§8.1) |
| `BLUETOOTH_LIST` | read | `Unprobed("bluetooth.list")` | Implemented | **IP + RL** | no recording site; `ServiceRole::Bluetooth` has no `role_channel()` entry (§6.2). Live plane exists on the target: `MSFT_NetAdapter` lists `蓝牙网络连接 2` / `Bluetooth Device (Personal Area Network) #2` |

Two related facts observed while closing the audit, both evidence that the tree is still moving:

* `SECURITY_STATUS` gained its probe mid-audit (`windows_hal.rs`,
  `for channel in ["channel.security", "security.status"]`) — it leaves the blind set (§5.2) but
  keeps the `RL` defect.
* `touchpad.caps`, `cpu.status`, `firmware.status`, `panel.identity`, `audio.endpoints/volume/mute`
  and `security.status` are now recorded; only `usb.ports`, `events.log`, `wifi.status`,
  `bluetooth.list`, `radio.*`, `update.local_package` and `update.history.ledger` remain unrecorded
  among `Implemented` rows.
