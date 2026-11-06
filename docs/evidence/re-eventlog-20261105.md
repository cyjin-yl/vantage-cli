# Lenovo event-log records — 2026-11-05 (contract), re-probed 2026-11-06

Target: `tb14p-ezra` (ThinkBook 21VG). Every probe in this document is
read-only: `wevtutil el/ep/qe`, `Get-WinEvent`, `Get-CimInstance Win32_Service`
and `reg query`. No channel is enabled, cleared or configured, no event is
written, and no registry value is written. Nothing here is a rendered vendor
message; the document records what the OS actually stores.

The contract was recovered on 2026-11-05 (the summary lives at the top of
`crates/lctrl-hal/src/event_log.rs` and `crates/lctrl-hal-win/src/event_log.rs`).
Every row below was re-observed live on 2026-11-06 through the encoded
PowerShell probes described in §9, so this document states only observed facts.

## 1. There is no Lenovo ETW channel, publisher or event source

| probe | command | result |
|---|---|---|
| channel list | `wevtutil el` filtered for `lenovo` | **0 lines** |
| publisher list | `wevtutil ep` filtered for `lenovo` | **0 lines** |
| registered event sources | `reg query HKLM\SYSTEM\CurrentControlSet\Services\EventLog /s /f Lenovo /k` | `End of search: 0 match(es) found.` |

So the Lenovo software on this SKU does not write through an ETW
manifest/publisher: there is no `Lenovo*` channel, no registered publisher, and
no `<channel>\<source>` key whose message file lives under a Lenovo path.
`docs/interfaces.md` event classes (`LENOVO_*_EVENT`) are WMI event classes, not
Windows event-log channels, and they are owned by the `events` module.

## 2. The only Lenovo records are classic `Application` entries

`wevtutil qe Application "/q:*[System[Provider[@Name='LenovoPcManagerService']]]" /c:2 /rd:true /f:xml`
returned two records verbatim:

```xml
<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='LenovoPcManagerService'/><EventID Qualifiers='0'>0</EventID><Version>0</Version><Level>4</Level><Task>0</Task><Opcode>0</Opcode><Keywords>0x80000000000000</Keywords><TimeCreated SystemTime='2026-11-03T18:09:41.0721605Z'/><EventRecordID>18163</EventRecordID><Correlation/><Execution ProcessID='5500' ThreadID='0'/><Channel>Application</Channel><Computer>TB14P-EZRA</Computer><Security/></System><EventData><Data>Service started/resumed</Data></EventData></Event>
<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='LenovoPcManagerService'/><EventID Qualifiers='0'>0</EventID><Version>0</Version><Level>4</Level><Task>0</Task><Opcode>0</Opcode><Keywords>0x80000000000000</Keywords><TimeCreated SystemTime='2026-11-03T14:05:06.7289704Z'/><EventRecordID>18050</EventRecordID><Correlation/><Execution ProcessID='6376' ThreadID='0'/><Channel>Application</Channel><Computer>TB14P-EZRA</Computer><Security/></System><EventData><Data>Service started/resumed</Data></EventData></Event>
```

| field | record 18163 | record 18050 |
|---|---|---|
| `Provider` | `LenovoPcManagerService` | `LenovoPcManagerService` |
| `EventID` (`Qualifiers`) | `0` (`0`) | `0` (`0`) |
| `Version` / `Level` / `Task` / `Opcode` | `0` / `4` / `0` / `0` | `0` / `4` / `0` / `0` |
| `Keywords` | `0x80000000000000` (classic event) | `0x80000000000000` |
| `TimeCreated` (UTC) | `2026-11-03T18:09:41.0721605Z` | `2026-11-03T14:05:06.7289704Z` |
| `EventRecordID` | `18163` | `18050` |
| `Execution` | process 5500, thread 0 | process 6376, thread 0 |
| `Channel` | `Application` | `Application` |
| `EventData/Data[0]` | `Service started/resumed` | `Service started/resumed` |

Level 4 is information. The `System` channel holds **no** record for this
provider (`wevtutil qe System` with the same XPath returned nothing), and
process 6376 no longer exists: the records outlive the writer.

## 3. The classic provider filter cannot find these records

| probe | command | result |
|---|---|---|
| classic filter | `Get-WinEvent -FilterHashtable @{LogName='Application'; ProviderName='LenovoPcManagerService'}` | error: `There is not an event provider on the localhost computer that matches "LenovoPcManagerService".` |
| XPath filter | `Get-WinEvent -LogName Application -FilterXPath "*[System[Provider[@Name='LenovoPcManagerService']]]" -MaxEvents 1` | the record above |

`ProviderName=` resolves the name through the registered-publisher table, which
has no entry for this string. `Provider[@Name=...]` only compares the attribute
stored in the record, so it is the **only** filter that finds these entries.
That is why the HAL builds
`*[System[Provider[@Name='…'] or Provider[@Name='…']]]` from discovered names
and never uses `ProviderName=`.

## 4. Provider discovery: the Lenovo services on this machine

`Get-CimInstance Win32_Service` filtered on `PathName -match Lenovo` returned
exactly these nine services (names and image paths verbatim):

| service key | `ImagePath` |
|---|---|
| `LAVService` | `"C:\Program Files (x86)\Lenovo\PCManager\5.1.210.10222\Security\service\LAVService.exe"` |
| `LenovoFnAndFunctionKeys` | `C:\WINDOWS\System32\DriverStore\FileRepository\lenovofnandfunctionkeys.inf_amd64_5e21bf389d23855a\LenovoUtilityService.exe` |
| `LenovoPcManagerService` | `"C:\Program Files (x86)\Lenovo\PCManager\5.1.210.10222\LenovoPcManagerService.exe"` |
| `LenovoVantageService` | `"C:\Program Files (x86)\Lenovo\VantageService\\5.1.2609.9\LenovoVantageService.exe"` |
| `LnvSvcFdn` | `"C:\Program Files (x86)\Lenovo\PCManager\5.1.210.10222\LnvSvcFdn.exe"` |
| `LnvVCamInstaller` | `"C:\Program Files\Lenovo\Smart Meeting\resources\Components\VirtualCameraService_x64_v3.3.2.13\VirtualCamera_Installer.exe"` |
| `SmartConnect` | `"C:\Program Files\Lenovo\Ready For Assistant\ReadyForService.exe"` |
| `UDCService` | `C:\WINDOWS\System32\drivers\Lenovo\udc\Service\UDClientService.exe` |
| `HRWSCCtrl` | `"C:\Program Files (x86)\Lenovo\PCManager\5.1.210.10222\Security\x64\wsctrl11.exe" /svc_run` |

Per-name XPath queries against `Application` and `System` (`/c:1`) produced
exactly one hit; every other name — the service key name **and** the image stem
spelling — is empty:

| candidate provider string | `Application` | `System` |
|---|---|---|
| `LenovoPcManagerService` | **hit** | 0 |
| `LAVService`, `LenovoUtilityService`, `LenovoVantageService`, `LnvSvcFdn`, `VirtualCamera_Installer`, `ReadyForService`, `UDClientService`, `wsctrl11`, `LNVDispatcherService` | 0 | 0 |

Consequence for the implementation: provider names are **discovered** from
`HKLM\SYSTEM\CurrentControlSet\Services\<service>\ImagePath` (Lenovo path ⇒
service key name and image stem) plus registered event sources whose
`EventMessageFile` lives under a Lenovo path. No name is hardcoded, and a newly
installed Lenovo service changes the query without a code change. The discovery
is bounded (64 names, 2048 registry children per level) and de-duplicated
case-insensitively.

## 5. Message rendering: empty, and it must stay empty

Same record, three renderings:

| probe | result |
|---|---|
| `Get-WinEvent … .Message` | `MessageLength=0`, `Message=''` |
| `Get-WinEvent … .Properties` | `Properties=1`, `[0]='Service started/resumed'` |
| `wevtutil qe … /f:text` | `Description: ` then `操作成功完成。` |

The `wevtutil` text formatter falls back to the system message table for event
id 0 (`ERROR_SUCCESS` = "the operation completed successfully"); that string is
generic OS text, not Lenovo content, and it is **not** what the record stores.
The record stores one `EventData/Data` string.

The HAL therefore never renders a message and never substitutes text for a
missing resource: it reports the raw `EventData` strings in record order, and an
empty list stays empty. This is the "empty message resource must not be replaced
by a synthesized message" rule, applied literally.

## 6. Volume, so a bound is a real bound

| probe | result |
|---|---|
| `wevtutil qe Application … /c:100 /rd:true` | 48 records for `LenovoPcManagerService` |
| newest record | `2026-11-03T18:09:41.0721605Z` (`EventRecordID` 18163) |
| oldest sampled record | `2026-09-11T07:05:33.5565718Z` |
| `System` channel | 0 records |

The oldest sample is the 48th newest record, i.e. the history extends further
back; the CLI cap (200) and the default (20) therefore do not need paging.

## 7. Implementation contract (as written)

`crates/lctrl-hal/src/event_log.rs`:

* `MAX_EVENT_LOG_RECORDS = 200`, `DEFAULT_EVENT_LOG_RECORDS = 20`,
  `normalize_event_log_limit` rejects `0` and anything above 200 with
  `InvalidArgument`.
* `EventLogControl::lenovo_events(limit) -> LenovoEventLogReport`; the report
  carries `channels`, `providers`, `events` (newest first, at most `limit`) and
  `limit`. Empty `events` with non-empty `providers` means "no records"; both
  empty means "no Lenovo provider/source discoverable".
* `EventLevel::from_raw` keeps the raw Windows value and the documented name
  (0 log_always … 5 verbose, otherwise `unknown`).
* `format_utc_millis` renders `TimeCreated` as `YYYY-MM-DDTHH:MM:SS.mmmZ`
  (Howard Hinnant civil-from-days, no calendar dependency).

`crates/lctrl-hal-win/src/event_log.rs` (`WindowsEventLog`):

* `EvtQuery(Application|System, xpath, EvtQueryChannelPath|EvtQueryReverseDirection)`;
  a channel that does not exist (`ERROR_EVT_CHANNEL_NOT_FOUND`) is skipped,
  never synthesized.
* `EvtNext` in batches of 16 with a 250 ms timeout, `EvtRender(EvtRenderEventValues)`
  with a growable buffer bounded at 256 KiB, `EvtClose` for every record handle
  including those dropped when the cap is reached mid-batch and those left in a
  batch whose record failed to render (the first failure is reported only after
  the batch has been closed), and RAII for the query handle and every registry
  key.
* Before any variant is read, `property_count * size_of::<EVT_VARIANT>()` must
  fit both `BufferUsed` and the buffer; a record that disagrees is refused with
  `ChannelUnavailable` instead of being read past.
* Rendered values: `ProviderName`, `EventID`, `Qualifiers`, `Level`,
  `TimeCreated` (FILETIME → Unix ms → UTC), `EventRecordId`, `Channel`, and the
  user-data variants from index 18 upward (strings and string arrays).
* Read-only by construction: no `EvtClearLog`, no `EvtSetChannelConfigProperty`,
  no `EvtSubscribe`, no `RegSetValue*`. Nothing in this path can clear a log or
  change a channel.
* Capability rows `events.log` and `channel.events`, recorded by a probe that
  only discovers provider names and opens/closes the `Application` query.

Registry/command surface: `CommandId::EVENTS_LOG` (`"events.log"`),
`Dependency::Win32EventLog` (Windows component `"Windows event log (wevtapi)"`),
one registry row `read_only(Runtime)` with `FeatureBinding::Capability("events.log")`,
`ServiceBinding::One(ServiceRole::Events)` and `DispatchSupport::Implemented`;
the CLI exposes `vantage events [--limit N]` and attaches the service in the
Windows composition root.

## 8. Not yet verified (parent-side acceptance)

* `vantage events` has **not** been run on the target in this session. The run
  (`vantage events --limit 2 --json`) and its comparison with §2 are parent
  acceptance, not something this document claims.
* `cargo check -p lctrl-hal-win --target x86_64-pc-windows-msvc` reports no
  error in `event_log.rs`; the crate currently fails in sibling modules that are
  mid-edit (`usb_ports.rs`, `radio.rs`, `storage.rs`, `touchpad_caps.rs`), which
  is why a whole-crate pass is still outstanding.
* The capability probe cost was measured only by inspection: it opens ~400
  service keys and the EventLog source keys, then one channel query. If the
  parent measures `info` latency on the target and finds it unacceptable, the
  source-scan can be limited to keys whose `ImagePath` mentions Lenovo first
  (the current order does exactly one `RegQueryValueExW("ImagePath")` per
  service key).

## 9. Reproduction

The probes ran as one encoded PowerShell script per round over SSH
(`powershell -NoProfile -EncodedCommand <base64 UTF-16LE>`), which avoids the
quoting traps in `wevtutil /q:` arguments. The commands, in order:

```powershell
(wevtutil el) | Select-String -SimpleMatch -Pattern 'lenovo'
(wevtutil ep) | Select-String -SimpleMatch -Pattern 'lenovo'
reg query "HKLM\SYSTEM\CurrentControlSet\Services\EventLog" /s /f Lenovo /k

wevtutil qe Application "/q:*[System[Provider[@Name='LenovoPcManagerService']]]" /c:2 /rd:true /f:xml
wevtutil qe System      "/q:*[System[Provider[@Name='LenovoPcManagerService']]]" /c:1 /rd:true /f:xml

Get-CimInstance Win32_Service | Where-Object { $_.PathName -match 'Lenovo' } |
  ForEach-Object { "{0} | {1}" -f $_.Name, $_.PathName }

# per candidate name:
wevtutil qe Application "/q:*[System[Provider[@Name='$n']]]" /c:1 /rd:true /f:xml

# filter semantics and message rendering:
Get-WinEvent -FilterHashtable @{LogName='Application'; ProviderName='LenovoPcManagerService'} -MaxEvents 3
Get-WinEvent -LogName Application -FilterXPath "*[System[Provider[@Name='LenovoPcManagerService']]]" -MaxEvents 1
wevtutil qe Application "/q:*[System[Provider[@Name='LenovoPcManagerService']]]" /c:1 /rd:true /f:text
```

Expected negative controls (all observed above):

* a provider name that is not a registered publisher makes
  `Get-WinEvent -FilterHashtable @{ProviderName=…}` fail, while
  `Provider[@Name=…]` still finds the record;
* `wevtutil el`/`ep` filtered for `lenovo` stay empty;
* the `System` channel query returns no record;
* a name that is neither a Lenovo service nor an image stem (`LNVDispatcherService`
  on this machine) returns no record, proving the query is not matching loosely.
