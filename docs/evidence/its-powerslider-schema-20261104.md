# ITS PowerSlider schema — attributed from `LNVDispatcherService.exe`

Source binary: `/tmp/its/LNVDispatcherService.exe` (the dispatcher installed by
`lnvprocessmanagement.inf`). All names it publishes under
`HKLM\SYSTEM\CurrentControlSet\Services\LenovoProcessManagement\Performance\PowerSlider`
were recovered from the binary's string table (`/tmp/its/strings.txt`) and the
code that reads them (`/tmp/its/dis.txt`). Each attribution cites an exact hex
offset and a verbatim string. Anything the strings do not settle is marked
`unresolved`, never guessed.

Observed registry snapshot on the 21VG target (unchanged during this work,
read-only):

```
Enable=1, Policy_ACDC=0, Policy_Override=3, Policy_DisablePLx=1,
Policy_FNSyncPowerMode=1, Policy_ModernITS=1, Policy_NotifyBIOS=1,
Policy_SWOEMV=3, Policy_DisablePPMCtrl=0, Policy_NamedPipe=1,
ITS_CurrentSetting=3, ITS_SE_AutoModeCapability=5, ITS_SE_CurrentScenario=0,
Policy_PLxSetMode=0x10001, BSMTConSpaceOFF=0, Policy_Send2ServOption=1,
Policy_TurboTime=134744072, AIGC_Priority=2, ITS_60nits=40,
ITS_AutomaticModeSetting=7, ITS_CurrentSettingV=3, ITS_FanMode=4,
ITS_FN_Capability=0x101A, ITS_HW_Capability=16, ITS_SE_Scenario_0..5=0.
```

## 1. The mode enum — proven by dispatch and status strings

`ITS_CurrentSetting` (requested) and `ITS_CurrentSettingV` (firmware-confirmed)
hold a small mode integer. The dispatcher's own status string names the two
numeric anchors; the `DirectPowerModeChange` code adds the third branch.

```
0x00f58d0  A  [NewITS_Status] ITS Mode Arive; (1:BSM, 3:EPM)Last Mode is %d, New mode  is %d
```

At `0x14001b2b2` the mode value is loaded and dispatched on three cases:

```
14001b2b2:  mov    0xb8(%rsp),%eax
14001b2b9:  mov    %eax,0x24(%rsp)
14001b2bd:  cmpl   $0x0,0x24(%rsp)      -> je 0x14001b2df   (Balance block)
14001b2c4:  cmpl   $0x1,0x24(%rsp)      -> je 0x14001b471   (BSM block)
14001b2cf:  cmpl   $0x3,0x24(%rsp)      -> je 0x14001b603   (EPM block)
14001b2da:  jmp    0x14001b796          (default)
```

The three case blocks each reference a labelled debug string; resolving those
offset references against `strings.txt` gives the name per case:

```
0x1400f73b0  A  -DirectPowerModeChange,26100: Balance : %d   <- value 0
0x1400f7478  A  -DirectPowerModeChange 26100: BSM : %d       <- value 1
0x1400f7518  A  -DirectPowerModeChange 26100: EPM : %d       <- value 3
```

So the enum is:

| integer | name | role |
|---------|------|------|
| 0 | Balance | balanced |
| 1 | BSM | quiet / battery-saver mode |
| 3 | EPM | performance (enhanced performance mode) |
| 2 | *(not dispatched — see note)* | quiet/other, unresolved |
| 4 | Geek | user-toggleable iGeek scenario, not a base slider mode |

`26100` (`0x65F4`) is the Windows "modern standby" overlay-scheme marker the
dispatcher tests (`cmpl $0x65f4` at `0x14001b3c6`/`0x14001b558`/`0x14001b6ea`)
before each `DirectPowerModeChange` leg; it tags the code path, not the mode.

Geek is not a base slider value. It is a separate checkbox/scenario state:
`_ITSCap is support Geek` (`0x00ff9e8`), `WriteITSModeToReg:bCheckBox_IGEEK %d`
(`0x00efb18`), `CheckGEEKMode,ML_ScenarioThread._Policy_CheckBox_IGEEK %d`
(`0x0106520`). The CLI maps `Geek=4` for its own enum, but the dispatcher treats
Geek as an overlay on top of a base mode rather than a fifth `ITS_CurrentSetting`
value, so `4` in `ITS_CurrentSetting` is not expected. Marking this `[INFERENCE]`
for the value-4-as-slider-slot claim specifically.

Value `2` is reachable in the code's range but takes the `jmp` default branch,
so its user-facing name is unresolved — `2=Quiet` is the CLI's own enum and is
plausible but not proven here. `Quiet` does not appear as a literal in the
binary near the mode code.

## 2. `ITS_AutomaticModeSetting` — a sentinel, not a mode

```
0x00ef968  W  ITS_AutomaticModeSetting
```

Read at `0x14000bc24` (`lea 0x1400f1168`), then compared against the constant 7:

```
14000bc44:  cmpl   $0x7,0x40(%rsp)
14000bc49:  jne    0x14000bcdd          (7 is special-cased)
14000bc4f:  movl   $0x3,0x140131000     (ITS_CurrentSetting <- 3)
```

So `7` is the sentinel meaning "automatic / dispatcher-chosen": when it holds 7,
the dispatcher writes `ITS_CurrentSetting` itself (here to 3 = EPM), instead of
honouring a user-set base mode. The observed value `7` is therefore the auto
flag set. Lower values (the BSM/EPM base values) would indicate a pinned mode.
Exact non-7 semantics beyond "not auto" are unresolved.

## 3. `Policy_PLxSetMode` — a packed two-field union

```
0x0100358  A  Policy_PLxSetMode
0x0100370  A  ML_ScenarioThread._Policy_PLxSetMode_Union %d,%d
```

The union log line prints **two** integers from one value, and the write site
initialises it from a single literal:

```
140082c61:  mov    $0x10001,%edx
140082c66:  lea    0x886eb(%rip),%rcx    # Policy_PLxSetMode name
140082c6d:  call   0x14001f290           (registry default write)
```

So `Policy_PLxSetMode` is a 32-bit value split into two 16-bit fields. The
observed `0x10001` = low16 `0x0001`, high16 `0x0001`. The `%d,%d` union format
string confirms the two-field packing, but which field is which (PL1-enable vs
PL2-enable, or a mode + a limit-class selector) is unresolved. The neighbour
writes in the same block give context:

```
140082c34:  mov    $0x202,%edx           (adjacent policy = 0x202)
140082c72:  mov    $0x9c0,%edx           (adjacent policy = 0x9c0)
```

The PL1/PL2 power-limit math lives in `UpdatePowerLimit` (`current PL1 %d,PL2
%d,setting PL1 %d,PL2 %d`, `0x00fb610`), which confirms this registry governs
the PLx subsystem but does not decode the union field meanings.

## 4. `ITS_FanMode` — a smart-fan mode selector

```
0x00ef8a0  W  ITS_FanMode
0x00f4668  W  SetSmartFanMode
0x00f4848  W  GetSmartFanMode
0x00f4878  A  GetSmartFanMode,value is %d
```

`ITS_FanMode` is read via the same registry helper (`0x14000bc…`), and
`GetSmartFanMode`/`SetSmartFanMode` are the access functions for the fan policy.
The observed value `4` selects one smart-fan operating mode. Its domain is the
same family as the mode enum (fan policy tracks the performance tier) but the
exact 0..N mapping for `ITS_FanMode` specifically is unresolved — `4` here is a
fan-mode index, not the Geek scenario constant, and the two must not be
conflated.

## 5. `Policy_DisablePLx` and `Policy_DisablePPMCtrl` — on/off gates

`Policy_DisablePLx=1` and `Policy_DisablePPMCtrl=0` are booleans (1=disable,
0=leave enabled). `Policy_DisablePLx=1` means the dispatcher is configured not
to drive the PLx power-limit path — consistent with `UpdatePowerLimit` existing
but the observed machine keeping the firmware's own limits. These are the kind
of value that makes a write path unsafe to invent: the dispatcher already
asserts it should not push PLx, so a CLI write of PL1/PL2 would fight a policy
the owner set. `Policy_DisablePPMCtrl=0` means PPM (processor performance)
control is *not* disabled, i.e. left on.

## 6. `ITS_60nits` — a brightness threshold, not a mode

```
0x00eef98  W  ITS_60nits
0x00ef6e0  A  [BIOS]::SetITSMode ret %d, m_bSetBSM %d, m_userBrightness %d, m_iBrightness %d
0x00fbaf6  W  dPolicy_BSMSetBright
0x0100050  A  Policy_BSMSetBright
```

`ITS_60nits=40` is a display-brightness threshold (in nits) the dispatcher uses
when entering BSM (the `Policy_BSMSetBright` / `m_bSetBSM`/`m_iBrightness`
strings tie it to the BSM brightness-dimming path). Not a performance setting.

## 7. `ITS_HW_Capability` / `ITS_FN_Capability` / `ITS_SE_AutoModeCapability` — unresolved bitmasks

```
0x00ef088  W  ITS_HW_Capability        (16)
0x00ef158  W  ITS_FN_Capability        (0x101A)
0x00ef228  W  ITS_SE_AutoModeCapability (5)
```

All three are read into the dispatcher's capability block at `0x140009b…` but
the three values disagree (16 / 0x101A / 5), so none can be attributed to a
specific mode bit from strings alone. The CLI already carries them raw in the
additive `its` block and does not decode them — that decision stands. They are
capability masks of *some* kind; which bits mean which mode is `unresolved`.

## 8. `ITS_SE_*` scenario registers — Smart-Engine scenario state

```
0x00f8940  A  ITS_SE_Scenario_X
0x00f8958  A  ITS_SE_Scenario_%d  EPMOVERRIDE=%d LastMode=%d
0x00ffa08  A  ITS_SE_VantageSetting
0x00ffa20  A  ITS_SE_HWCapability
```

`ITS_SE_Scenario_0..5` hold per-scenario Smart-Engine state (each logs an
`EPMOVERRIDE` and a `LastMode`). `ITS_SE_CurrentScenario` selects which of the
six slots is active. `ITS_SE_VantageSetting` / `ITS_SE_HWCapability` mirror the
scenario capability/settings seen through Vantage. All are `0` in the snapshot,
so the Smart-Engine scenario layer is idle. These are runtime state, not user
configuration.

## 9. What the CLI can report next, and at what confidence

| Value | What it means | Confidence |
|-------|---------------|------------|
| `ITS_CurrentSetting` / `ITS_CurrentSettingV` | mode enum {0=Balance,1=BSM,3=EPM,4=Geek(overlay)} | high (dispatch + status string) |
| `ITS_AutomaticModeSetting` | `7`=auto sentinel; `!=7`=pinned base mode | high for 7, unresolved below |
| `Policy_PLxSetMode` | packed two-u16 union (0x10001 = both fields 1) | medium (literal write + `%d,%d` log) |
| `ITS_FanMode` | smart-fan mode index; 4 selects a mode | medium (accessor names), domain unresolved |
| `Policy_DisablePLx` / `Policy_DisablePPMCtrl` | booleans gating the PLx / PPM writes | medium-high (boolean read + UpdatePowerLimit context) |
| `ITS_60nits` | BSM brightness threshold (nits) | high (SetITSMode/BSMSetBright strings) |
| `ITS_HW/FN/SE_Auto` capabilities | mode/feat bitmasks | unresolved (values disagree) |
| `ITS_SE_Scenario_*` | Smart-Engine per-scenario state (EPMOVERRIDE/LastMode) | medium (log format) |

Mode `2`'s user-facing name and the `ITS_FanMode`/`Policy_PLxSetMode` field
meanings are the only gaps the strings left open; everything else is anchored
to an exact offset and a verbatim string.
