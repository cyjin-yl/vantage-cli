# ITS `Policy_*` decode — `LNVDispatcherService.exe` (2026-11-05, read-only)

This note attributes the `PowerSlider` `Policy_*` values the dispatcher publishes
but the CLI previously only carried raw: `Policy_PLxSetMode`,
`Policy_FanSpeedAddress`, `Policy_APMinSTDFan(_CPU/_GPU)`,
`Policy_TurboValue`/`Policy_TurboTime`/`Policy_EPPTurbo` and the
`Policy_PowerBucket`/`Policy_CPPC`/`Policy_CFQC`/`Policy_CFQC1` tables.

Source binary: `/tmp/its/LNVDispatcherService.exe` (1,338,200 bytes, image base
`0x140000000`), the dispatcher installed by `lnvprocessmanagement.inf` and run as
the `LenovoProcessManagement` service. Disassembly below is `objdump -d` output
(`/tmp/its/dis.txt`), strings carry the pair *file offset → RVA*
(`/tmp/its/strings.txt`). Every claim cites an address; anything the code does
not settle is marked **unresolved**, never guessed.

Every value lives under the key string at `0x14010ab30`:

```
HKLM\SYSTEM\CurrentControlSet\Services\LenovoProcessManagement\Performance\PowerSlider
```

## 0. Read paths (proven, and read-only on our side)

| helper | role | evidence |
|---|---|---|
| `0x14001e1a0` | scalar read with `cbData = 8` into the caller's buffer | `movl $0x8,0x48(%rsp)` at `0x14001e1e4` |
| `0x14001e4e0` | scalar read with `cbData = 4` into the caller's buffer | `movl $0x4,0x48(%rsp)` at `0x14001e524`; used with the 8-byte scratch `0x1401b0280` |
| `0x14001e680` | scalar read with `cbData = 4` plus an extra stack argument | `movl $0x4,0x48(%rsp)` at `0x14001e6c9` |
| `0x14001e940` | `REG_SZ` table: 520-byte buffer (`movl $0x208`) + decimal scan | §6 below |
| `0x14001f290` | write default `REG_DWORD` | `RegSetValueExW(..., REG_DWORD)` |
| `0x14001f6a0` | write default string | `RegSetValueExW(..., r9d = 1 = REG_SZ)` at `0x14001f786` |

All three readers open the key with `KEY_READ` (`r9d = 0x20019`) through
`ADVAPI32!RegOpenKeyExW` (IAT `0x1400ef128`) and read with
`ADVAPI32!RegQueryValueExW` (IAT `0x1400ef120`). The default-string writer proves
the curve tables are stored **`REG_SZ`**, which is why `perf policy` reads them
with `RRF_RT_REG_SZ`.

The dispatcher's `cbData` is only a buffer bound, not the stored type: the
values it seeds itself are written as `REG_DWORD` (`RegSetValueExW` with
`REG_DWORD` at `0x14001f290`'s call site) and the repo's existing
`its`/`Policy_ModernITS` reads of this same key already succeed as `REG_DWORD`.
`perf policy` therefore reads scalars with `RRF_RT_REG_DWORD` and keeps a
4-byte bound; a value stored as some other type fails the read and is reported
as an `issue` instead of being silently reinterpreted.

The loader lives in one function (`0x14006fbd0`–`0x14007071f`): each value is
read into a **static singleton** (the `ML_ScenarioThread`/ITS config object, base
`0x1401b0f70`), so the loader uses absolute addresses while consumers read the
same field through `+offset`. That is why a direct cross-reference search on a
field address alone under-reports consumers; the offsets used below were found
that way.

## 1. `Policy_PLxSetMode` — two u16 lanes, AC and DC

Read (`0x140070507`, `REG_DWORD`, destination `0x1401b1a64` = object `+0xaf4`),
logged with the union format at `0x140101b70`:

```
0x140070529: mov 0x1401b1a64,%eax ; shr $0x10 ; and $0xffff  -> r8d (2nd %d)
0x140070537: mov 0x1401b1a64,%ecx ;            and $0xffff  -> edx (1st %d)
0x140070548: lea 0x140101b70,%rcx     # "ML_ScenarioThread._Policy_PLxSetMode_Union %d,%d"
```

so the low half is one lane and the high half the other. Which lane is AC is
proven by the call site that feeds the PLx applier:

```
0x14003702e: cmpl $0x5,0x500(%rax)      ; ITS mode must be >= 5 for this path
0x140037046: movzbl 0x5e(%rax),%eax     ; object +0x5e  = 0x1401b0fce
0x14003704a: test %eax,%eax ; je 0x140037064
0x140037053: mov 0xaf4(%rax) ; and $0xffff        -> AC uses the LOW  half
0x140037069: mov 0xaf4(%rax) ; shr $0x10 ; and …  -> DC uses the HIGH half
0x14003709d: call 0x14003c1f0(object, lane, &out, 0)
```

and `0x1401b0fce` is the AC-online flag, written from `GetSystemPowerStatus`:

```
0x14006b170: call *0x1400ef328        # KERNEL32!GetSystemPowerStatus
0x14006b183: movb $0x1,0x1401b13cd    # AC line
0x14006b191: mov %al,0x1401b0fce
0x14006b1b3: movb $0x0,0x1401b13cd    # battery
0x14006b1c1: mov %al,0x1401b0fce
```

Lane value → apply procedure (inside `0x14003c1f0`, lane homed at `0x58(%rsp)`):

```
0x14003c761: cmpl $0x0,0x58(%rsp) ; jne 0x14003c7ba
0x14003c780: call 0x14003da30     ; lane 0: paired write, records object +0x188/+0x18c
0x14003c7ba: cmpl $0x1,0x58(%rsp) ; jne 0x14003cef5
0x14003c7e4: cmpl $0x3,0x30(%rsp) ; lane 1: up to 3 attempts of a single-value write
0x14003c83b: call 0x1400188f0     ; (type 2, value)
```

`0x1400188f0` maps the type through the table at `0x1400f5350`
(`[0x02010000, 0x02020000, 0x01020000, 0x01010000]`) and forwards to
`0x140018290`, i.e. the GAMEZONE data write path; type 2 selects `0x01020000`.
Lane 0 goes through `0x14003da30` → `0x14008c480`, the local write path.

Consequences the CLI reports:

* `raw` plus `ac` and `dc`, never one collapsed number.
* Default (per-project default write `0x140082c61: mov $0x10001`) and the live
  target value are both `0x00010001` = AC 1 / DC 1.
* When **both** lanes are 1 the ITS PLx worker is not scheduled at all
  (`0x14002db7e`/`0x14002dba1` compare each lane with 1 and jump to the
  no-op return at `0x14002dca7`; otherwise the task is queued at `0x14002dbed`).
* `Policy_DisablePLx == 1` (loaded at `0x140071430`, compared with 1 at
  `0x140071449`, stored to object `+0x67` = `0x1401b0fd7`) makes the applier
  return before doing anything (`0x14003c215` → log `0x1400fcda8`
  "AI Seemless change, Direct Return").
* **Unresolved:** the member names of the vendor union (`_Policy_PLxSetMode_Union`
  is all the binary publishes). The lanes are therefore reported as `ac`/`dc`
  with the raw value beside them, and no meaning is invented for a lane value.

## 2. `Policy_APMinSTDFan`, `_GPU`, `_CPU` — enable + clamped percentages

```
0x14006fe43: lea "Policy_APMinSTDFan"      -> 0x1401b100b   (DWORD read, low byte used)
0x14006fe6f: lea "Policy_APMinSTDFan_GPU"  -> 0x1401b100c
0x14006fec3: lea "Policy_APMinSTDFan_CPU"  -> 0x1401b1010
0x14006fe91: cmpl $0x64,0x1401b100c ; jbe keep ; else movl $0x32   (100 / 50)
0x14006fee5: cmpl $0x64,0x1401b1010 ; jbe keep ; else movl $0x32
0x14006fe63: jne 0x14006ff35        ; enable read failed -> thresholds not read
0x14006ff28: lea 0x1401016d8        # "_Policy_APMinSTDFan %d ,GPU value %d , CPU value %d  "
```

So: `enable` = low byte of `Policy_APMinSTDFan` (only tested, never interpreted);
each threshold is a percentage kept for `0..=100` and replaced by **50** when the
stored value is larger, or when the value cannot be read at all
(`movl $0x32` on the failure path). Live target value of the enable DWORD is not
recorded in earlier notes; the decode reports it verbatim.

## 3. `Policy_FanSpeedAddress` — read, stored, never consumed

```
0x14006fc47: lea "Policy_FanSpeedAddress" ; call 0x14001e4e0 (REG_DWORD)
0x14006fc6f: mov %eax,0x1401b0f8c     # object +0x1c, low dword
0x14006fc77: movl $0x0,0x1401b0f8c    # zero when the read fails
```

No reader exists: the address `0x1401b0f8c` appears only in those two stores,
and scanning all 83 functions that take the config object base
(`lea … # 0x1401b0f70`) for an `0x1c(%reg)` field access finds none. The value is
therefore reported raw with no interpretation — the dispatcher reads it, the
shipped binary never uses it.

## 4. `Policy_TurboValue` — EPP value with a `0xff` sentinel

```
0x140070674: movl $0xa,0x1401b1a5c     # seed 10 before the read
0x140070696: lea "Policy_TurboValue"   -> 0x1401b1a5c (object +0xaec)
0x1400706c5: lea 0x140101c88           # "ML_ScenarioThread._Policy_TurboValue_Union %d"
```

Sentinel and use, from the EPP update path:

```
0x14003666e: cmpl $0xff,0xaec(%rax) ; jne …      # 0xff == default EPP
0x14003667a: lea 0x1400fcac0  # "_Policy_TurboValue_Union.TurboValue == uDefaultEPP"
0x140039d6b: add 0xaec(%rcx),%eax                 # value is added into the EPP computation
0x140039cd4/…: "UpdateTurboMode,_PreviousEPP %d , _CurrentEPP default"
```

`0xff` means "use the default EPP", so it is reported as a decoded flag with the
raw value still visible.

## 5. `Policy_TurboTime` and `Policy_EPPTurbo` — packed turbo parameters

### `Policy_TurboTime` (4×u8)

```
0x140070555: and $0xffffff ; or $0x05000000    # seed byte3 = 5
0x14007056b: and $0xffff00ff ; or $0x300
0x140070581: and $0xff00ffff ; or $0x30000
0x140070597: and $0xffffff00 ; or $0x3
0x140070601: lea "Policy_TurboTime" -> 0x1401b1a58 (object +0xae8)
0x1400705ef: lea 0x140101ba8  # "ML_ScenarioThread._Policy_TurboParam_UnionValue %d,%d,%d,%d"
```

The seed is `{byte0=3, byte1=3, byte2=3, byte3=5}`; the per-project default
write is `0x05050505` (`0x140082bf3`). Readers found:

| lane | evidence |
|---|---|
| byte2 (bits 16..24) — **turbo time** | `0x1400693a4`, `0x1400695ca`, `0x140069b78`; max'd against `Policy_EPPTurbo` bits 16..24 at `0x1400662e2`; reset to 5 by `0x140079309` (`and $0xff00ffff ; or $0x50000`) |
| byte0 (bits 0..8) | `0x1400699c2`, maximum-tracked against `0x1401b1c50` |
| byte1, byte3 | no reader in `.text` |

### `Policy_EPPTurbo`

```
0x140066239: lea "Policy_EPPTurbo" -> 0x1401b1a10 (object +0xaa0)
0x140066280: shr $0x18 ; and $0xf  -> object 0x1401b1114     # bits 24..28
0x140066383/0x140066399: cmp $0x5                            # nibble compared with 5
0x1400663cb: cmp $0x1 (jbe)                                  # and with 1
0x1400662a8/0x1400662d4: shr $0x10 ; and $0xff               # bits 16..24 = turbo time
0x1400664ab: shr $0x1c ; and $0x1                            # bit 28
0x1400664c3: lea 0x140104380  # "UpdateVariable_EPPTurbo,not turbo to APM."
0x1400664fc: lea 0x1401043b0  # "EPPTurbo_Union 0x%x "
0x14006651a: lea 0x1401043c8  # "EPPTurbo_Union read fail,status %u,last err %u"
```

Decoded: bits 24..28 a 4-bit mode nibble (compared against 5 and 1), bits 16..24
the same turbo-time quantity as `Policy_TurboTime` byte2 (the effective value is
the **larger** of the two, written to `0x1401b1110`), bit 28 the
"not turbo to APM" flag. Bits 0..16 and 29..32 have no reader and stay raw.

Live target values: `Policy_TurboTime = 134744072 = 0x08080808`
(byte2 = 8 → turbo time 8), `Policy_TurboValue = 20`,
`Policy_EPPTurbo = 553934735 = 0x21045f8f` → mode nibble 1, turbo time 4
(effective `max(4, 8) = 8`), bit 28 clear.

## 6. Curve tables — `REG_SZ` number runs, not key/value maps

Reader (`0x14001e940`): opens the key read-only, `RegQueryValueExW` into a
520-byte buffer (`movl $0x208,0x48(%rsp)`), logs
`Read_REG_Policy_PowerBucket:%ls,len = %d` (`0x1400f8248`, call `0x14001ea70`),
then scans the string:

```
0x14001ea7c: char index j = 0 ; slot index k = 0
0x14001ea98: cmp count,k ; jge done
0x14001eab3: c = wchar[j]
0x14001eabd/0x14001eacc: '0' <= c <= '9' ?
0x14001ead1: dest[k] = dest[k]*10 + (c - '0') ; j++ ; continue
0x14001eb0c: (non-digit) j++ ; k++ ; continue
```

The scanner is **sequential**: the numbers before the colons in `x:y;` are *not*
indices, they are the first element of each pair, so `0:15;10:30;` flattens to
`[0, 15, 10, 30, …]` and stops as soon as `k` reaches the array capacity.

| value | read site | capacity | destination | log / gate |
|---|---|---|---|---|
| `Policy_PowerBucket` | `0x1400714cb` | 12 (`mov $0xc`, `0x1400714bd`; buffer `0x30` bytes) | `0x1401b1bfc..0x1401b1c28` | log `0x1401025f0` `CPU_PowerBucket[11]/[1]`; applied only when `arr[11] != 0 && arr[1] > 0` (`0x14007150e`, `0x140071525`); override log `0x140102640` "Override CPU_Bucket Curve %d , %d , %d , %d" |
| `Policy_CPPC` | `0x1400716b0` | 16 (`mov $0x10`, `0x1400716a2`; buffer `0x40`) | `0x1401b17e0[i]`: low byte = `arr[2i]`, byte `<<16` = `arr[2i+1]` | log `0x140102680` `EPP_Curve[13]/[1]`; gate `arr[13] > 0 \|\| arr[1] > 0`; override log `0x1401026c0` labels the pair `0_AC, 0_DC, …` |
| `Policy_CFQC` | `0x140071b9d` | 16 | `0x1401b1820[i]`: low u16 = `arr[2i] * 100`, high u16 = `arr[2i+1] * 100` (`imul $0x64`, `and $0xffff`) | log `0x140102830` `CPUFreq_Curve`; gate `arr[0] > 0 \|\| arr[1] > 0`; override log `0x140102880` prints the low halves labelled `_DC` |
| `Policy_CFQC1` | `0x140071d98` | 16 | `0x1401b1820[i] + 4` (same ×100 scaling) | log `0x1401028f0` `CPUFreq1_Curve`; same gate |

Published default strings in the binary (the dispatcher writes these when a value
is missing):

```
0x14010a440  0:15;10:30;25:45;38:50;40:70;60:80;70:100;      PowerBucket (7 pairs)
0x14010a3d0  50:80;50:72;50:72;40:60;40:50;40:50;30:50;30:50  CPPC (8 pairs)
0x14010a5e0  30:15;30:15;30:20;30:25;30:30;30:40;30:40;30:40;
0x14010a6a0  99:99;99:99;90:90;80:80;70:70;60:60;50:50;50:50;
0x14010b0f0  80:80;70:70;70:70;60:60;60:60;50:50;50:50;50:50;
0x14010b1c0  0:20;0:20;0:25;0:30;0:35;0:50;0:50;0:50;          CFQC  (8 pairs)
0x14010b220  0:25;0:25;0:30;0:35;0:40;0:60;0:60;0:60;          CFQC1 (8 pairs)
```

Note the first line: the dispatcher's **own** `Policy_PowerBucket` default holds
14 numbers against a 12-slot array, so the vendor reader keeps
`[0,15,10,30,25,45,38,50,40,70,60,80]` and drops the last pair. That is a
property of the vendor code, not a decode choice: because a short table would be
a guess, `perf policy` refuses a list whose number count differs from the array
capacity and reports the raw text plus the counts instead.

**Unresolved (deliberately not invented):** what a curve position physically
means beyond the labels the binary prints (`CPU_Bucket Curve`, `EPP Curve` with
per-point AC/DC, `Frequency Curve` with `_DC`). The tables are reported as opaque
firmware tables: slots and pairs verbatim, no index semantics.

## 7. What the CLI now does with this

`vantage perf policy` (`crates/vantage-cli`, decoded in
`crates/lctrl-hal-win/src/its_policy.rs`, pure rules in
`crates/lctrl-core/src/performance.rs`) reads the eleven values above through
`RegGetValueW` only, and reports:

| value | reported |
|---|---|
| `Policy_PLxSetMode` | `raw`, `ac`, `dc` |
| `Policy_FanSpeedAddress` | raw dword (no consumer in the binary) |
| `Policy_APMinSTDFan(_GPU/_CPU)` | `enable`, `gpu`, `cpu` after the 0..=100/50 rule |
| `Policy_TurboValue` | `raw`, `default_epp` |
| `Policy_TurboTime` | `raw`, `turbo_time`, `byte0`, `byte1`, `byte3` |
| `Policy_EPPTurbo` | `raw`, `mode_nibble`, `turbo_time`, `not_turbo_to_apm` |
| the four tables | `source`, `capacity`, `slots`, `pairs` |

Absent / unreadable / undecodable stay distinct (§ module docs in
`its_policy.rs`), and every refused value is echoed with its raw text.

No write path exists on this surface: the registry reader used here has only
`read_dword`/`read_string`, both `RegGetValueW` against `KEY_READ`, and the
Windows test fixture records (and fails on) any write attempt.
