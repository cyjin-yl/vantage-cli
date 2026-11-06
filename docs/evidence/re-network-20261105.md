# Wi-Fi link state and Bluetooth device list — owner recovery and `wifi status` / `bluetooth list` contract — 2026-11-05

Target: `TB14P-EZRA` / LENOVO 21VG (ThinkBook 14 G8+ IPH), Windows 11 Pro for
Workstations `10.0.26300`, firmware `SWCN22WW`, session `TB14P-EZRA\ezra` (elevated, over
SSH/Tailscale).
Scope: the two **read-only** network verbs of this wave.
- `wifi status` — SSID / BSSID / signal (percent and dBm) / band / channel / security of the live
  WLAN association.
- `bluetooth list` — the local adapter plus every known Bluetooth device with its address, class,
  paired/connected flags and last seen/used times.

Out of scope and untouched: radio soft/hard state and `radio off|on` (owner: the Radio API slice,
`docs/evidence/re-radio-20261105.md`), Wi-Fi saved-profile security (`wifi security`, existing
netsh slice in `crates/lctrl-hal-win/src/wifi_security.rs`).

This document was written **before** the implementation, per the wave method: contract first, then
code that only covers what the live channel actually exposes.

## 1. Verdict: which owner answers which fact

| fact | owner | why not the alternatives |
|---|---|---|
| SSID, BSSID, profile, signal %, RX/TX rate, PHY type, auth/cipher algorithm of the live association | **`wlanapi!WlanQueryInterface` opcode 7** (`wlan_intf_opcode_current_connection`) | `netsh wlan show interfaces` carries the same fields but is a second text channel with localized labels; it is kept as the independent oracle (§3.1) |
| interface link state | **opcode 6** (`wlan_intf_opcode_interface_state`) | WinRT `Radio.State` is an aggregate per-radio state, not a link state |
| channel number | **opcode 8** (`wlan_intf_opcode_channel_number`) | — |
| band and RSSI in dBm | **`wlanapi!WlanGetNetworkBssList` for the associated SSID** (`lRssi`, `ulChCenterFrequency`) | `ulChCenterFrequency` from the BSS entry is the only documented frequency source. Channel-number→band inference is **refused**: 6 GHz reuses the 2.4 GHz channel numbers (1…233 across both), so channel 36 is not proof of 5 GHz on a Wi-Fi 7 adapter |
| local Bluetooth adapter identity, known device list, per-device connected/remembered/authenticated flags, last seen/used | **`bluetoothapis!BluetoothFindFirstRadio` + `BluetoothGetRadioInfo` + `BluetoothFindFirstDevice`/`BluetoothFindNextDevice`** | see §1.1 — this is the only *non-scanning* enumerator that reports `fConnected` at all |
| independent cross-check of the Bluetooth list | **PnP** (`SetupAPI`/`Get-PnpDevice`) and the **WinRT `Windows.Devices.Enumeration` AEP enumerator** | both were probed live (§3.3, §3.4) and agree with the Win32 enumerator; PnP additionally supplies `DEVPKEY_Bluetooth_LastConnectedTime`. Neither is required at runtime |

### 1.1 Why `BluetoothFindFirstDevice` and not the WinRT Devices enumerator

Both were recovered live. The WinRT AEP enumerator gives exactly three properties of interest
(`System.Devices.Aep.IsConnected`, `IsPaired`, `DeviceAddress`) but needs `RoGetActivationFactory`
plus `IAsyncOperation<DeviceInformationCollection>`, `IVectorView<DeviceInformation>`,
`IMapView<HSTRING, IInspectable>` and `IPropertyValue` hand-bound in Rust. The Win32 enumerator
delivers the same facts (proven equal in §3.4) through one documented header and no COM, and it
additionally returns the local radio identity, the class of device, and the last seen/used times.
The Devices enumerator therefore remains a **cross-check only**; if a later slice needs
container/AEP-specific facts it has to bind that COM surface separately.

PnP alone is **not** sufficient for the list: it exposes one device node per *service*
(`BTHENUM\{0000110b-…}` A2DP, `BTHENUM\{00001105-…}` OPP, `BTHLEDEVICE\{0000180f-…}` GATT, …) —
87 such nodes were enumerated live (§3.2) for 5 real devices. Reporting those as "devices" would
be a per-service row explosion, and PnP has no connection-state property.

## 2. Header authority

`C:\Windows\System32\wlanapi.dll` is `10.0.26100.8875`; the machine carries SDK includes
`10.0.22621.0` and `10.0.26100.0`, so `10.0.26100.0` is the matching authority for every structure
below (`um\wlanapi.h`, `shared\wlantypes.h`, `shared\windot11.h`, `um\bluetoothapis.h`).

```c
/* um\wlanapi.h */
typedef struct _WLAN_INTERFACE_INFO {
    GUID  InterfaceGuid;                            /* offset  0 */
    WCHAR strInterfaceDescription[WLAN_MAX_NAME_LENGTH]; /* offset 16, 256 wchars */
    WLAN_INTERFACE_STATE isState;                   /* offset 528 */
} WLAN_INTERFACE_INFO;                              /* sizeof 532 */

typedef struct _WLAN_INTERFACE_INFO_LIST {
    DWORD dwNumberOfItems;                          /* offset 0 */
    DWORD dwIndex;                                  /* offset 4 */
    WLAN_INTERFACE_INFO InterfaceInfo[1];           /* offset 8 */
} WLAN_INTERFACE_INFO_LIST;

typedef struct _WLAN_CONNECTION_ATTRIBUTES {
    WLAN_INTERFACE_STATE   isState;                 /* offset   0 */
    WLAN_CONNECTION_MODE   wlanConnectionMode;      /* offset   4 */
    WCHAR strProfileName[WLAN_MAX_NAME_LENGTH];     /* offset   8, 256 wchars */
    WLAN_ASSOCIATION_ATTRIBUTES wlanAssociationAttributes; /* offset 520 */
    WLAN_SECURITY_ATTRIBUTES    wlanSecurityAttributes;    /* offset 588 */
} WLAN_CONNECTION_ATTRIBUTES;                       /* sizeof 604 */

typedef struct _WLAN_ASSOCIATION_ATTRIBUTES {
    DOT11_SSID        dot11Ssid;      /* 520 .. 556  { ULONG uSSIDLength; UCHAR ucSSID[32]; } */
    DOT11_BSS_TYPE    dot11BssType;   /* 556 */
    DOT11_MAC_ADDRESS dot11Bssid;     /* 560 .. 566 */
    DOT11_PHY_TYPE    dot11PhyType;   /* 568 */
    ULONG             uDot11PhyIndex; /* 572 */
    WLAN_SIGNAL_QUALITY wlanSignalQuality; /* 576, percent */
    ULONG             ulRxRate;       /* 580, "The unit for Rx/Tx rate is Kbits/second" */
    ULONG             ulTxRate;       /* 584 */
} /* 68 bytes */

typedef struct _WLAN_SECURITY_ATTRIBUTES {
    BOOL bSecurityEnabled;            /* 588 */
    BOOL bOneXEnabled;                /* 592 */
    DOT11_AUTH_ALGORITHM  dot11AuthAlgorithm;   /* 596 */
    DOT11_CIPHER_ALGORITHM dot11CipherAlgorithm;/* 600 */
} /* 16 bytes */

/* 10.0.26100 layout — note lRssi, added before uLinkQuality */
typedef struct _WLAN_BSS_ENTRY {
    DOT11_SSID   dot11Ssid;            /*   0 */
    ULONG        uPhyId;               /*  36 */
    DOT11_MAC_ADDRESS dot11Bssid;      /*  40 */
    DOT11_BSS_TYPE    dot11BssType;    /*  48 */
    DOT11_PHY_TYPE    dot11BssPhyType; /*  52 */
    LONG         lRssi;                /*  56  dBm */
    ULONG        uLinkQuality;         /*  60  percent */
    BOOLEAN      bInRegDomain;         /*  64 */
    USHORT       usBeaconPeriod;       /*  66 */
    ULONGLONG    ullTimestamp;         /*  72 */
    ULONGLONG    ullHostTimestamp;     /*  80 */
    USHORT       usCapabilityInformation; /* 88 */
    ULONG        ulChCenterFrequency;  /*  92  kHz */
    WLAN_RATE_SET wlanRateSet;         /*  96 .. 352 */
    ULONG        ulIeOffset;           /* 352  offset of the IE blob, from the entry start */
    ULONG        ulIeSize;             /* 356 */
} WLAN_BSS_ENTRY;                      /* fixed part 360, then ulIeSize bytes of IE, 8-byte aligned */

/* shared\wlantypes.h — enum values, verbatim */
DOT11_AUTH_ALGO_80211_OPEN = 1, DOT11_AUTH_ALGO_80211_SHARED_KEY = 2, DOT11_AUTH_ALGO_WPA = 3,
DOT11_AUTH_ALGO_WPA_PSK = 4, DOT11_AUTH_ALGO_WPA_NONE = 5, DOT11_AUTH_ALGO_RSNA = 6,
DOT11_AUTH_ALGO_RSNA_PSK = 7, DOT11_AUTH_ALGO_WPA3 = 8 /* "means WPA3 Enterprise 192 bits" */,
DOT11_AUTH_ALGO_WPA3_SAE = 9, DOT11_AUTH_ALGO_OWE = 10, DOT11_AUTH_ALGO_WPA3_ENT = 11;
DOT11_CIPHER_ALGO_NONE = 0, WEP40 = 1, TKIP = 2, CCMP = 4, WEP104 = 5, BIP = 6, GCMP = 8,
GCMP_256 = 9, CCMP_256 = 10, BIP_GMAC_128 = 11, BIP_GMAC_256 = 12, BIP_CMAC_256 = 13,
WPA_USE_GROUP = RSN_USE_GROUP = 0x100, WEP = 0x101;
dot11_BSS_type_infrastructure = 1, independent = 2, any = 3;
dot11_phy_type_ofdm = 4 (11a), hrdsss = 5 (11b), erp = 6 (11g), ht = 7 (11n), vht = 8 (11ac),
dmg = 9 (11ad), he = 10 (11ax), eht = 11 (11be);
/* um\wlanapi.h */ wlan_intf_opcode_radio_state = 4, _interface_state = 6,
_ current_connection = 7, _channel_number = 8, _msm_start = 0x10000100, _statistics = 0x10000101,
_rssi = 0x10000102;
wlan_interface_state_not_ready = 0, _connected = 1, _ad_hoc_network_formed = 2, _disconnecting = 3,
_disconnected = 4, _associating = 5, _discovering = 6, _authenticating = 7.

/* um\bluetoothapis.h */
typedef struct _BLUETOOTH_RADIO_INFO {   /* sizeof 520 */
    DWORD dwSize;                        /*   0 */
    BLUETOOTH_ADDRESS address;           /*   8, union { ULONGLONG ullLong; UCHAR rgBytes[6]; } */
    WCHAR szName[BLUETOOTH_MAX_NAME_SIZE]; /* 16, 248 wchars */
    ULONG ulClassofDevice;               /* 512 */
    USHORT lmpSubversion;                /* 516 */
    USHORT manufacturer;                 /* 518 (BTH_MFG_* assigned number) */
} BLUETOOTH_RADIO_INFO;

typedef struct _BLUETOOTH_DEVICE_INFO {  /* sizeof 560 */
    DWORD dwSize;                        /*   0 */
    BLUETOOTH_ADDRESS Address;           /*   8 */
    ULONG ulClassofDevice;               /*  16 */
    BOOL  fConnected;                    /*  20 */
    BOOL  fRemembered;                   /*  24 */
    BOOL  fAuthenticated;                /*  28 */
    SYSTEMTIME stLastSeen;               /*  32, UTC */
    SYSTEMTIME stLastUsed;               /*  48, UTC */
    WCHAR szName[BLUETOOTH_MAX_NAME_SIZE]; /* 64, 248 wchars */
} BLUETOOTH_DEVICE_INFO;

typedef struct _BLUETOOTH_DEVICE_SEARCH_PARAMS { /* sizeof 40 */
    DWORD dwSize; BOOL fReturnAuthenticated; BOOL fReturnRemembered; BOOL fReturnUnknown;
    BOOL fReturnConnected; BOOL fIssueInquiry; UCHAR cTimeoutMultiplier; HANDLE hRadio;
} BLUETOOTH_DEVICE_SEARCH_PARAMS;   /* fIssueInquiry = FALSE: no inquiry/scan is ever issued */
```

`WLAN_MAX_NAME_LENGTH` = `L2_PROFILE_MAX_NAME_LENGTH` = 256. `BLUETOOTH_MAX_NAME_SIZE` = 248.

An offset is only accepted by the implementation when the buffer the operating system returns
*proves* it: `sizeof(WLAN_CONNECTION_ATTRIBUTES)` must equal the returned size (604 observed), and a
BSS entry is only decoded when its own `ulIeOffset` equals the fixed-part size the layout predicts
(§5).

## 3. Live receipts (2026-11-05/06, read-only)

### 3.1 Independent text oracle: `netsh wlan show interfaces`

```
Name : WLAN      Description : Intel(R) Wi-Fi 7 BE213 160MHz
GUID : a281055e-60b2-4bc5-89dc-d1b6bbca7712   Physical address : 34:fd:70:17:74:8d
State : connected
SSID : BUAA-Mobile      AP BSSID : b0:f9:63:24:1d:03
Band : 5 GHz            Channel : 36
Authentication : WPA2-Enterprise     Cipher : CCMP
Receive rate (Mbps) : 216   Transmit rate (Mbps) : 162
Signal : 78%                Rssi : -66
Profile : BUAA-Mobile       Radio type : 802.11ac
```

### 3.2 Bluetooth PnP shape (`Get-PnpDevice`), read-only

`Get-PnpDevice -Class Bluetooth` returned **75** nodes. They decompose into four kinds:

| instance-id prefix | meaning | count |
|---|---|---|
| `BTHENUM\DEV_<12 hex>` | one node per **known classic device** per enumerator parent, friendly name = device name | 6 (3 devices × 2 parents) |
| `BTHLE\DEV_<12 hex>` | one node per **known LE identity** per enumerator parent | 6 (3 identities × 2 parents) |
| `BTHENUM\{service-guid}_…`, `BTHLEDEVICE\{…}` | one node per *service* per device per parent | 54 |
| `BTH\MS_BTHBRB`/`MS_BTHLE`/`MS_BTHPAN`/`MS_RFCOMM`, `IBTPCIBUS`, `IBTLEBPDODEVICE`, `PCI\VEN_8086&DEV_E476&SUBSYS_01118086&REV_01\3&11583659&0&A7` | protocol enumerators and the Intel adapter halves | 9 |

The doubling is real and matters: the machine has **two** `BTH\MS_BTHBRB` enumerator instances
(instance parents `5&224A8231` — `IsPresent=True` — and `5&1A631BBB` — `IsPresent=False`), so every
device node exists once per parent. Enumerating PnP as the device list would report each device
twice as well as once per service.

Per-device PnP properties that corroborate the Win32 enumerator (read from
`BTHENUM\DEV_0CBD75ABF23A\…`):

```
DEVPKEY_Device_FriendlyName        = OPPO Find X8s
DEVPKEY_Bluetooth_DeviceAddress    = 0CBD75ABF23A
DEVPKEY_Bluetooth_ClassOfDevice    = 5915148        (= 0x5a420c, identical to §3.3)
DEVPKEY_Bluetooth_LastConnectedTime= 11/05/2026 06:27:10
DEVPKEY_Device_IsPresent           = True
{3B2CE006-5E61-4FDE-BAB8-9B8AAC9B26DF} 8 = Bluetooth#Bluetooth34:fd:70:17:74:91-0c:bd:75:ab:f2:3a
```

The last property is the WinRT AEP id of the same device, which shows the local radio address
(`34:fd:70:17:74:91`) independently of the Win32 enumerator.

### 3.3 Win32 Bluetooth enumerator (the implemented plane)

```
sizeof(BLUETOOTH_RADIO_INFO)=520  sizeof(BLUETOOTH_DEVICE_INFO)=560  sizeof(SEARCH_PARAMS)=40
BluetoothFindFirstRadio  -> hFind valid, hRadio valid, GetLastError=0
BluetoothGetRadioInfo    -> rc=0
    address=34:fd:70:17:74:91 (ullLong 0x34fd70177491)  name='<machine name>'
    ulClassofDevice=3031308 (0x2e43cc)  lmpSubversion=0x52d6  manufacturer=2 (Intel)

BluetoothFindFirstDevice(hRadio = NULL, fIssueInquiry = FALSE,
                         fReturnAuthenticated = TRUE, fReturnRemembered = TRUE,
                         fReturnConnected = TRUE, fReturnUnknown = FALSE)
  0c:bd:75:ab:f2:3a  OPPO Find X8s              cod=0x5a420c conn=0 rem=16 auth=8  lastSeen=2026-11-04 lastUsed=2026-11-04
  00:a4:1c:36:51:6c  OPPO Enco Free4…           cod=0x240404 conn=0 rem=16 auth=8  lastSeen=2026-11-06 lastUsed=2026-11-06
  98:40:16:b5:53:08  thinkplus-TH62             cod=0x240404 conn=0 rem=16 auth=8  lastSeen=2026-04-03 lastUsed=2026-04-03
  (BluetoothFindNextDevice ends with GetLastError=259 ERROR_NO_MORE_ITEMS)

same call with fReturnUnknown = TRUE adds
  2c:ca:16:1e:84:ed  …MacBook Air               cod=0x38010c conn=0 rem=0  auth=0
  f8:ac:65:f6:ea:d1  WESTBROOK0                 cod=0x0a010c conn=0 rem=0  auth=0
```

Two facts the implementation must respect:

- **`hRadio = NULL` is documented ("NULL == all radios will be searched") and works.** The
  machine exposes two `BTH\MS_BTHBRB` enumerator instances (parents `5&224A8231` present,
  `5&1A631BBB` not present); enumerating per radio would either duplicate rows or miss devices, so
  the implementation passes `NULL` and enumerates once.
- **`fIssueInquiry = FALSE` is mandatory for this verb.** It is the only parameter that starts an
  inquiry (a radio-level scan). With it FALSE the call is a pure cache read.
- The three `BOOL` fields come back as `0` and *non-zero non-1* values (`16` and `8`). Per Win32
  `BOOL` semantics the implementation treats **0 = false, any non-zero = true**. Whether `16`/`8`
  are internal flags or a masked encoding is not settled by this evidence: `fRemembered` and
  `fAuthenticated` were non-zero for exactly the three devices the WinRT enumerator reports as
  `IsPaired = True`, and zero for exactly the two it reports as `IsPaired = False`, so the
  four-value sample cannot separate "remembered" from "authenticated". Reported as two booleans,
  never merged into one.

### 3.4 Cross-check: WinRT AEP enumerator and PnP agree

`Windows.Devices.Bluetooth.BluetoothDevice.GetDeviceSelector()` with
`DeviceInformation.FindAllAsync` (properties `IsConnected`, `IsPaired`, `DeviceAddress`):

```
Bluetooth#Bluetooth34:fd:70:17:74:91-0c:bd:75:ab:f2:3a  OPPO Find X8s        IsPaired=True  IsConnected=False  0c:bd:75:ab:f2:3a
Bluetooth#Bluetooth34:fd:70:17:74:91-00:a4:1c:36:51:6c  OPPO Enco Free4…     IsPaired=True  IsConnected=False  00:a4:1c:36:51:6c
Bluetooth#Bluetooth34:fd:70:17:74:91-98:40:16:b5:53:08  thinkplus-TH62       IsPaired=True  IsConnected=False  98:40:16:b5:53:08
```

The plain protocol selector (`System.Devices.DevObjectType:=5 AND
System.Devices.Aep.ProtocolId:="{E0CBF06C-CD8B-4647-BB8A-263B43F0F974}"`) returns **5** endpoints —
the same three plus `2c:ca:16:1e:84:ed` (MacBook Air, `IsPaired=False`) and `f8:ac:65:f6:ea:d1`
(`IsPaired=False`), i.e. exactly the extra two that `fReturnUnknown = TRUE` adds in §3.3.

`IsConnected = False` for all five matches `fConnected = 0` for all five. **A connected device was
never observed** (nothing was connected during the run), so the `fConnected = 1` path is implemented
from the header contract and is *unproven* (§6).

### 3.5 WLAN API, live

```
WlanOpenHandle(2) -> rc=0 negotiated=2
WlanEnumInterfaces -> rc=0, 1 interface
  guid=a281055e-60b2-4bc5-89dc-d1b6bbca7712 desc='Intel(R) Wi-Fi 7 BE213 160MHz' isState=1
  opcode 6  (interface_state)     rc=0 size=4   value=1          (wlan_interface_state_connected)
  opcode 8  (channel_number)      rc=0 size=4   value=36
  opcode 7  (current_connection)  rc=0 size=604
     isState@0=1  mode@4=4 (wlan_connection_mode_auto_connect)
     profile@8='BUAA-Mobile'  ssidLen@520=11  ssid@524='BUAA-Mobile'
     bssType@556=1 (infrastructure)  bssid@560=b0:f9:63:24:1d:03
     phy@568=8 (dot11_phy_type_vht = 802.11ac, = netsh "Radio type")
     phyIdx@572=4  signal@576=78 (percent, = netsh "Signal 78%")
     rx@580=243000 kbps  tx@584=81000 kbps     (moments differ from netsh's 216/162 Mbps)
     secEnabled@588=1  oneX@592=1
     auth@596=6 (DOT11_AUTH_ALGO_RSNA)  cipher@600=4 (DOT11_CIPHER_ALGO_CCMP)
```

`auth = 6` + `cipher = 4` with `oneX = 1` is WPA2-Enterprise/CCMP, which is what netsh reports for
the same association; the enum values themselves come from `wlantypes.h` (§2), so the name mapping
is not inferred from the label.

Negative control, deliberately run and rejected: opcode `0x10000101` (= `wlan_intf_opcode_statistics`,
*not* RSSI) returns **rc=0 with a 1232-byte buffer whose first word is 0**. A slice that assumed
"RSSI is a LONG" would have silently reported 0 dBm. The documented RSSI opcode is `0x10000102`;
this implementation does not use it at all — the RSSI it reports is the BSS entry's `lRssi`.

### 3.6 BSS list: variable-size entries

```
WlanGetNetworkBssList(interface, ssid = current SSID, dot11_BSS_type_infrastructure,
                      bSecurityEnabled = FALSE) -> rc=0
  dwTotalSize=544  dwNumberOfItems=1
  entry[0]: ssidLen=11 ssid='BUAA-Mobile' bssid=b0:f9:63:24:1d:03
            lRssi@56 = -66          (= netsh "Rssi : -66")
            uLinkQuality@60 = 78    (= netsh "Signal : 78%")
            ulChCenterFrequency@92 = 5180000 (kHz) = 5180 MHz  -> 5 GHz (= netsh "Band : 5 GHz")
            ulIeOffset@352 = 360, ulIeSize@356 = 172
            entry stride = align8(360 + 172) = 536;  8 + 536 = 544 = dwTotalSize
```

Two traps this receipt pins down:

- **Entries are variable-size.** `(dwTotalSize - 8) / dwNumberOfItems` is *not* the entry size in
  general (here it happens to be 536 because there is one entry); the walker must advance by
  `align8(ulIeOffset + ulIeSize)` from each entry's own header.
- **The pre-26100 layout misreads on this build.** Reading the same buffer with the old offsets
  (`ulIeOffset@344`, `freq@84`) yields `freq = 31288657` — garbage. The 360-byte fixed part is
  required, and is proven by the entry's own `ulIeOffset`.

Session safety: `WlanGetNetworkBssList` reads the cached BSS list; `WlanScan` is never called. The
probe ran inside the very SSH session whose only uplink is this association, and the session
survived.

## 4. Readback candidates tested and rejected (do not use)

| candidate | evidence |
|---|---|
| channel number → band | ambiguous by construction: 6 GHz and 2.4 GHz share channel numbers; a Wi-Fi 7 adapter makes the shortcut wrong |
| `WlanQueryInterface` opcode `0x10000101` as RSSI | returns a 1232-byte buffer, first word 0 (§3.5); the value would be a fabrication |
| `netsh wlan show interfaces` as the *primary* source for `wifi status` | it works and is kept as an oracle, but it is a localized text channel; the numeric fields (`Band`, `Rssi`, `Connected Akm-cipher` as OUI-encoded strings) would have to be re-parsed per locale |
| PnP `BTHENUM`/`BTHLEDEVICE` nodes as the device list | one row per *service*: 87 nodes for 5 devices (§3.2) |
| `HKLM\SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Devices` | 67 address-keyed subkeys that hold link keys; no names, no connection state. Reading it would touch secret material for no benefit — not read |
| `BluetoothFindFirstDevice` with `fIssueInquiry = TRUE` | starts an inquiry; excluded by scope (read-only, no scan) |

## 5. Implemented plane and fail-closed rules

`crates/lctrl-hal-win/src/wifi_status.rs`
- `WlanOpenHandle(2)`, `WlanEnumInterfaces`, `WlanQueryInterface`(6, 7, 8),
  `WlanGetNetworkBssList`, `WlanFreeMemory`, `WlanCloseHandle`. **No** `WlanScan`,
  **no** `WlanSetInterface`, **no** `WlanConnect`.
- Fail-closed: non-zero `rc` from open/enumerate → `Unsupported`/`ChannelUnavailable`; a query whose
  returned size differs from `size_of::<WLAN_CONNECTION_ATTRIBUTES>()` is rejected rather than
  reinterpreted; `uSSIDLength > 32` is rejected; an unnameable enum value keeps its raw number and
  yields no name (`None`), never a guess.
- Band is derived **only** from `ulChCenterFrequency` (2 400–2 500 MHz → 2.4 GHz, 5 150–5 895 MHz →
  5 GHz, 5 925–7 125 MHz → 6 GHz, 57 000–71 000 MHz → 60 GHz, anything else → no band).
- A BSS entry is decoded only when `ulIeOffset` equals the fixed-part size the layout predicts
  (360 for the 26100 layout) **and** walking every entry by `align8(ulIeOffset + ulIeSize)` lands
  exactly on `dwTotalSize`; otherwise RSSI/band stay absent for that interface.
- The BSS query is filtered to the associated SSID and `dot11_BSS_type_infrastructure`, and is only
  issued for an interface whose state is `connected`.

`crates/lctrl-hal-win/src/bluetooth.rs`
- `BluetoothFindFirstRadio`/`BluetoothFindNextRadio`/`BluetoothFindRadioClose`,
  `BluetoothGetRadioInfo` (+ `CloseHandle` on the radio handle), and
  `BluetoothFindFirstDevice`/`BluetoothFindNextDevice`/`BluetoothFindDeviceClose` with
  `fIssueInquiry = FALSE`, `hRadio = NULL`, `fReturnAuthenticated = TRUE`,
  `fReturnRemembered = TRUE`, `fReturnConnected = TRUE`, **`fReturnUnknown = FALSE`**.
  With `fIssueInquiry = FALSE` the enumerator reads the local cache; `fReturnUnknown = FALSE`
  keeps devices that were merely seen in a past inquiry (the two passers-by of §3.3/§3.4) out of a
  machine inventory. Duplicate addresses are collapsed, so a device is listed once even when the
  service reports it per enumerator parent.
- Fail-closed: `ERROR_NO_MORE_ITEMS` (259) from `BluetoothFindFirstRadio` is "no radio on this
  machine" and is reported as an empty adapter list plus an explicit reason, not as an error;
  any other failure is an error; a `SYSTEMTIME` that is not a valid calendar instant (all-zero =
  never) becomes `None` rather than `1970-01-01`.
- `ulClassofDevice` is reported raw **and** decoded to the Bluetooth assigned-number major device
  class (bits 12–8). The two live values decode to `Phone` (0x5a420c) and `Audio/Video` (0x240404),
  consistent with the device names; minor classes are not decoded.
- The local adapter name is reported as-is (this machine names its radio after the computer); it is
  never used as an identifier — `address` is.

## 6. What is observed versus what is still unrun

Observed live (all read-only, §3): every input the two verbs consume — interface enumeration, link
state, current-connection attributes, channel, BSS list with center frequency and RSSI, local radio
info, and both enumerator result sets (3 paired devices, 5 including unknown ones) — plus the three
cross-checks.

**Not run, therefore not claimed:**
- The Rust implementations themselves. No build, test or binary run was performed by this slice;
  the parent owns that. The first real check is `vantage wifi status` against the netsh oracle in
  §3.1 and `vantage bluetooth list` against `Get-PnpDevice`/the AEP list in §3.4.
- `fConnected = 1`: never observed (§3.4). The boolean is passed through, so a connected device will
  report `connected: true` only if the API does.
- Bluetooth radio switched off: `BluetoothFindFirstRadio` would return
  `ERROR_NO_MORE_ITEMS`; the "no radio" branch is implemented but was not exercised (switching the
  radio off is another slice's write verb and is out of scope here).
- Non-elevated execution: the observed session was elevated. Every call used is documented as
  user-level (`WlanOpenHandle` client version 2, the Bluetooth enumerators); no administrator
  requirement was measured.
