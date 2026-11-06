# Keyboard backlight — recovered channel (`ACPI KBLC` over `\\.\EnergyDrv`)

Target: TB14P-EZRA (Lenovo ThinkBook 14 G8+ IPH, 21VG), Windows 11 build 26100.
Date: 2026-11-05. Work item: KBD-BACKLIGHT (`kbd.backlight`).

Until now `vantage kbd backlight` had no channel on this machine:
`LENOVO_LIGHTING_DATA` / `LENOVO_LIGHTING_METHOD` answer `不支持` and every
`LENOVO_GAMEZONE_DATA` method answers `无效的对象`
(`docs/evidence/channel-misread-sweep-20261104.md`). This note records the
channel that does work: the Lenovo **Virtual Power Controller** ACPI method
`KBLC`, reached through the `AcpiVpc` driver's vendor IOCTL.

Everything below is either read from the installed binaries, quoted from the
public VPC2004 contract, or produced by **read-only** IOCTL calls (query and
get). **No write was issued**: the set path is implemented from the recovered
argument encoding and is left to the parent's acceptance run.

## 1. Device and driver

```text
ACPI\VPC2004\0 | Lenovo ACPI-Compliant Virtual Power Controller | Status OK
Service  acpivpc  Start=3 (demand)  ImagePath=
  \SystemRoot\System32\DriverStore\FileRepository\acpivpc.inf_amd64_07db38bc9e02b27d\AcpiVpc.sys
File     C:\Windows\System32\DriverStore\FileRepository\acpivpc.inf_amd64_07db38bc9e02b27d\AcpiVpc.sys
Size     52800 bytes
Version  15.11.30.14 (CompanyName Lenovo, "ACPI Virtual Power Controller Driver")
SHA-256  c02b879e601c1689425bdd71ec6fc2ccf66d75436e973ab4f209d45473e58c95
```

The same driver is the one that creates `\\.\EnergyDrv`, the device the CLI
already opens for the battery channels (`docs/interfaces.md` §11). The keyboard
channel therefore needs no new service, no service start, and no elevation
beyond what `battery status` already requires.

PDB path embedded in the image:
`C:\home\jenkins\agent\workspace\GAMING_Driver-EM_release-jenkins\build\x64\Release\AcpiVpc.pdb`.

## 2. IOCTL dispatch (proof)

The driver's `DispatchDeviceControl` compares the control code against a table
of vendor codes and jumps to a per-feature handler. Keyboard-backlight entries
(`objdump -d` of the image, addresses are image VAs):

```text
140011bda  cmp ecx, 0x83102144     je 0x140011df1     ; -> handler 0x140001fa4
140011c0a  cmp ecx, 0x83102154     je 0x140011d2f     ; -> handler 0x1400020f8
140011c16  cmp ecx, 0x83102158     je 0x140011cfa     ; -> handler 0x140001e50
140011c22  cmp ecx, 0x8310215c     je 0x140011cb9     ; -> handler 0x14000f000 (GAPD, 10-byte out)
```

Every keyboard handler requires `InputLength >= 4` and `OutputLength >= 4`
(`cmp ebx,0x4` / `cmp r12d,0x4`, else `STATUS_BUFFER_TOO_SMALL 0xc0000023`).

### 2.1 `0x83102144` evaluates ACPI `KBLC`

Handler `0x140001fa4` builds a 12-byte input buffer and issues
`IOCTL_ACPI_EVAL_METHOD_EX` (`0x32C004`) to its ACPI device:

```text
140002016  mov dword [rbp+0x27], 0x49696541      ; ACPI eval input signature ("AeiI")
140002029  mov dword [rbp+0x2b], 0x434c424b      ; "KBLC"  <- little-endian method name
14000200e  mov r9d, 0xc                          ; input length = 12
140002020  mov ecx, 0x32c004                     ; IOCTL_ACPI_EVAL_METHOD_EX
             ; dword [rbp+0x2f] = the caller's input u32 (the method argument)
```

i.e. `{u32 signature, char method[4], u32 argument}` — a 4-character method name
relative to the device plus a 32-bit integer argument. The documented
`ACPI_EVAL_INPUT_BUFFER_SIMPLE_INTEGER_EX` reserves 256 bytes for the name and
passes a 64-bit argument (268 bytes); the driver sends the compact 12-byte form,
and the live calls below prove the ACPI driver accepts it.

The reply is validated as an ACPI evaluation **output** buffer before anything
is returned to the caller (`0x1400020a2`…`0x1400020cb`):

```text
cmp qword [rbp-0x11], 0x14        ; output length >= 20
cmp dword [rbp+0x0f], 0x426f6541  ; "AeoB" = ACPI_EVAL_OUTPUT_BUFFER_SIGNATURE
cmp dword [rbp+0x17], 0           ; Length field must be nonzero
cmp word  [rbp+0x1b], 0           ; ACPI_METHOD_ARGUMENT.Type must be 0 (Integer)
mov eax, [rbp+0x1f] -> *out       ; u32 integer result
```

So the IOCTL contract is exactly:

| direction | size | content |
| --- | --- | --- |
| input | 4 bytes | method argument (u32, little-endian) |
| output | 4 bytes | method result (u32, little-endian) |

A wrong `Type` yields `0xc014000f`; a malformed buffer yields `0xc0000001`; a
missing method yields the ACPI evaluation error — the driver never fabricates a
value, so a successful call is the firmware's own answer.

### 2.2 The other two keyboard entries

The same handler shape (4-byte in, 4-byte out, identical output validation)
exists for two more method names:

```text
0x83102158 -> 0x140001e50, name 0x414c424b "KBLA" (0x140001ed5)
0x83102154 -> 0x1400020f8, name 0x524c424b "KBLR" (0x14000217d)
```

Their argument semantics are **not** recovered, and on this machine both
return `0` for argument `0`. Since `KBLC` already answers for every command the
keyboard needs, `KBLA`/`KBLR` are not used by the implementation and are
recorded only so a future slice does not have to rediscover the dispatch table.

## 3. `KBLC` argument encoding (VPC2004 contract)

`KBLC` is the control method the Linux `ideapad-laptop` driver uses for
keyboard backlight on VPC2004 machines; the encoding below is that driver's
published contract (patch "platform/x86: ideapad-laptop: Add support for
keyboard backlights using KBLC ACPI symbol", v3, 2023-08-27) and the DSDT
disassembly of the same method published for a Yoga 9i Gen 7
(`lotusroot1/lenovo-yoga-backlight-config`):

| argument | meaning | returns |
| --- | --- | --- |
| `0x1` | query the keyboard group/config (`KBD_BL_QUERY_TYPE`) | `KBGC \| 1`; `5` = tristate, `7` = tristate + auto |
| `(group << 4) \| 0x2` | get current state (`KBD_BL_COMMAND_GET`) | `KBGS \| 1`; state = `(ret >> 1) & 7` |
| `(state << 16) \| (group << 4) \| 0x3` | set state (`KBD_BL_COMMAND_SET`) | reply is not a state word; the state is re-read with GET |

`group = (query & ~1) >> 1`, so `5 -> 2` (states `0..2`) and `7 -> 3`
(states `0..3`). The DSDT rejects a command whose group field does not match
`KBGC`; in practice that shows up as a **zero** result rather than as an error
(see the live `0x22` call below), which is why a zero answer must never be
decoded as "backlight off".

State domain (`KBGS` bits 2:1):

| state | meaning |
| --- | --- |
| `0` | off |
| `1` | low |
| `2` | high |
| `3` | auto (ambient-light controlled; only when the query returned `7`) |

Kernel cross-check (same symbols, `drivers/platform/x86/ideapad-laptop.c`):
`KBD_BL_QUERY_TYPE 0x1`, `KBD_BL_COMMAND_GET 0x2`, `KBD_BL_COMMAND_SET 0x3`,
type selector in bits 7:4, brightness in bits 19:16, `max_brightness = 2` for
tristate. The kernel maps state `3` back to `0` because sysfs LED has no "auto".

## 4. Live, read-only observations (2026-11-05, target)

PowerShell probe: `CreateFileW("\\\\.\\EnergyDrv", 0xC0000000, 0, NULL,
OPEN_EXISTING)` then `DeviceIoControl(0x83102144, in u32, out 4)`.

```text
KBLC(0x1)  = ok out=0x00000007   KBGC|1 -> tristate+auto, group 3, states 0..3
KBLC(0x32) = ok out=0x00000001   KBGS|1 -> KBGS=0, state 0 = off
KBLC(0x22) = ok out=0x00000000   group 2 mismatches KBGC=6 -> zero, no status
```

Conclusions actually supported by these three calls:

- The machine implements `KBLC`, and it answers over `0x83102144` — the
  `LENOVO_*` WMI planes being dead does not make the feature unavailable.
- The keyboard group is `3` and the firmware advertises four states
  (off / low / high / auto), so a three-state assumption would be wrong.
- The group selector really is enforced: a mismatched group yields `0`, not a
  state. A `0` result is therefore "no status for this group", never "off".

Not observed (and not claimed): the effect of a `SET`, the physical backlight
changing, Fn+Space transitions, the auto state's sensor behaviour, and whether
the EC reports the change back through the EnergyDrv
`0x1000 KeyboardBacklightStatus` event (`docs/02-power-battery.md` §11.2).
Those need the acceptance run.

## 5. Implementation boundary (`crates/lctrl-hal-win/src/kbd_backlight.rs`)

- `WindowsKeyboardBacklight<I: IoctlTransport>` speaks only `0x83102144`
  with 4-byte in/out; it re-queries the layout on every operation instead of
  caching it, because the firmware owns the keyboard group.
- Read: query → get → decode. `0` (no status), a non-integer-sized reply, or a
  state outside the advertised group are `InvalidArgument`, never "off".
- Write: `set_backlight` refuses non-static lighting effects
  (`Unsupported: kbd.backlight.effect`, this interface has no effect concept),
  validates `level <= max_state`, and on `DryRun` performs **only** query + get
  (no set IOCTL at all). A commit issues one set and then polls the GET path
  (10 × 50 ms); on mismatch it restores the captured previous state and
  re-verifies it, reporting `FirmwareRejected` if the rollback also fails.
- `WindowsKeyboardSources<primary, secondary>` keeps the WMI
  `LENOVO_LIGHTING_METHOD` controller as a fallback for machines that publish
  that plane. Fallback happens only for `Unsupported` / `ChannelUnavailable`;
  a device-level error from the VPC channel is propagated, and if the fallback
  fails too the primary error is the one reported.
- Capability: `kbd.backlight` is now `Available` when the `KBLC` query answers
  with a supported keyboard type, or when `LENOVO_LIGHTING_DATA` still exposes
  an instance (RGB machines).

CLI: `vantage kbd backlight <off|low|high|auto>`; the raw firmware level
`0..3` is still accepted, matching `docs/04-peripherals.md` §2.0's documented
`{0..3}` surface.

## 6. Remaining risk

The set encoding is the public VPC2004/kernel contract and is consistent with
this machine's query and get answers, but it has not been executed here. If the
firmware uses a different write layout, the write is verified by readback and
rolled back, so the failure mode is a reported `VerifyMismatch` rather than a
silent wrong state. First acceptance run should therefore be
`kbd backlight high --dry-run`, then `kbd backlight high`, then
`kbd backlight off`, each with the previous level captured in the output.
