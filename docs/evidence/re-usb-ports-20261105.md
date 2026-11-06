# USB port inventory (`usb ports`) — source recovery — 2026-11-05

Scope: wave-5 item *USB port inventory + power-delivery capability (read-only)*.
Question answered here: **which live Windows surfaces give per-port controller id, attached
device, negotiated speed and port power capability on this SKU, what they actually return, and
what `vantage usb ports` may therefore claim.**

## 1. Verdict

| surface | status on 21VG | used for `usb ports` |
|---|---|---|
| `GUID_DEVINTERFACE_USB_HUB` (`{f18a0e88-c30c-11d0-8815-00a0c906bed8}`) via SetupAPI | 2 hub interfaces (`USB\ROOT_HUB30\4&32F967F1&0&0`, `USB\ROOT_HUB30\4&196F3462&0&0`) | yes — hub discovery |
| `IOCTL_USB_GET_NODE_INFORMATION` (`0x00220408`, kernel `USB_GET_NODE_INFORMATION`) | works, returns hub descriptor | yes — port count, bus-powered, `wHubCharacteristics` |
| `IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX` | works on all 17 ports | yes — connection, speed, address, device descriptor |
| `IOCTL_USB_GET_NODE_CONNECTION_DRIVERKEY_NAME` | works on occupied ports, `ERROR_NO_SUCH_DEVICE` on empty | no (superseded by the child-devnode join, §4.4) |
| `IOCTL_USB_GET_DESCRIPTOR_FROM_NODE_CONNECTION` (config) | works on occupied ports | yes — device `bMaxPower` |
| `IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX_V2` | **fails: Win32 error 87 on every port and every input length tried** | **no** (§5) |
| `Win32_USBController` / PnP class `USB` | 2 × `USBXHCI` (`E47D`, `E431`) + 1 × `Usb4HostRouter` (`E434`) | yes — controller identity |
| `USB4\ROOT_DEVICE_ROUTER` (+ sibling `USB4\VIRTUAL_POWER_PDO`) | present, **no** queryable port/PD surface in user mode | presence only (§6) |
| UCM/UCSI PD PPM `ACPI\USBC000` | present (`UcmUcsiAcpiClient`, firmware 1.2) | presence only — **no user-mode connector interface** (§6) |
| `Win32_PortConnector` (SMBIOS Type 9) | boilerplate template rows, not real ports | **no** (§6.3) |
| EnergyDrv `0x831020e8` | already RE-proven, AC/battery always-on **enable bits only** | no — no per-port PD/BC1.2 data |

Consequence: `usb ports` reports the real controller → hub → port tree with the negotiated speed
read from the hub driver, the attached device's own USB descriptors and declared power draw, and
reports **power delivery / BC1.2 as unavailable with the measured reason**. It never infers
wattage from port colour, marketing copy or the SMBIOS connector table.

## 2. Target

| field | value (read-only) |
|---|---|
| model | `Win32_ComputerSystem.Model = 21VG`, `Manufacturer = LENOVO` |
| OS | `Microsoft Windows 11 专业工作站版`, version `10.0.26300`, build `26300` |

Probes: `/tmp/usb-probe1.ps1` … `usb-probe4.ps1`, copied to `C:\Users\ezra\` and run with
`powershell -NoProfile -ExecutionPolicy Bypass -File`; output files fetched back. Everything
below is copied from those captures; no value is reconstructed from marketing material.

## 3. Controllers and hubs (live)

```
PCI\VEN_8086&DEV_E47D&SUBSYS_386317AA&REV_01\3&11583659&0&A0
    Intel(R) USB 3.20 可扩展主机控制器 - 1.20 (Microsoft), service USBXHCI, status OK
    root hub USB\ROOT_HUB30\4&196F3462&0&0  (12 ports)
PCI\VEN_8086&DEV_E431&SUBSYS_385217AA&REV_01\3&11583659&0&68
    Intel(R) USB 3.20 可扩展主机控制器 - 1.20 (Microsoft), service USBXHCI, status OK
    root hub USB\ROOT_HUB30\4&32F967F1&0&0  (5 ports)
PCI\VEN_8086&DEV_E434&SUBSYS_385517AA&REV_01&USB4_MS_CM\3&11583659&0&6B
    USB4(TM) Host Router (Microsoft), service Usb4HostRouter, status OK,
    driver 10.0.26100.9549, DEVPKEY_Device_FirmwareVersion = 10.1,
    child USB4\ROOT_DEVICE_ROUTER&VID_8086&PID_E434\4&35A04B34&0&0  (firmware 10.1)
      sibling USB4\VIRTUAL_POWER_PDO\4&35A04B34&0&0  (PDO name \Device\00000072, no exposed data)
ACPI\USBC000\0   UCM-UCSI ACPI Device, class UCM, service UcmUcsiAcpiClient, status OK,
                 DEVPKEY_Device_BiosDeviceName = \_SB.UBTC,
                 DEVPKEY_Device_FirmwareVersion = 1.2, driver 10.0.26100.8972
```

`Win32_USBController` returns the same three entries (`E47D`, `E434`, `E431`), each with
`ConfigManagerErrorCode = 0`.

Hub-to-controller parentage is a property read, not an inference: both root hubs report
`DEVPKEY_Device_Parent` = their xHCI PCI instance id.

## 4. Per-port data (live), with the offset proof

### 4.1 Hub node information

`IOCTL_USB_GET_NODE_INFORMATION`, 4096-byte output buffer, hub opened read-only
(`CreateFileW(path, 0, FILE_SHARE_READ|WRITE, OPEN_EXISTING)`), `ret = 76` bytes:

```
hub 4&32F967F1: 00 00 00 00 09 29 05 00 00 02 00 00 ff 00 ...
hub 4&196F3462: 00 00 00 00 09 29 0c 00 00 02 00 00 00 ff ff ...
```

Decoded against `USB_NODE_INFORMATION` (packed): `NodeType` = 0 (`UsbHub`); `USB_HUB_DESCRIPTOR`
at offset 4 → `bLength = 0x09`, `bDescriptorType = 0x29`, `bNumberOfPorts` = **5** / **12**;
`wHubCharacteristics` = `0x0000` (ganged power switching, global over-current — USB 2.0 §11.11.2.1);
`DeviceRemovable`/`PortPwrCtrlMask` follow with `(ports/8)+1` bytes each (`00` / `ff` for 5 ports,
`0000` / `ffff` for 12). `ret = 76 = 4 + sizeof(USB_HUB_INFORMATION)` fixes `HubIsBusPowered`
at offset 75 → **0** (self-powered) on both root hubs.

### 4.2 Port connection information

`IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX`, `ConnectionIndex` = port, `ret = 35` for an empty
port and `35 + 11·pipes` for an occupied one (11 = `sizeof(USB_PIPE_INFO)`), which pins the packed
layout: `ConnectionStatus` @31, `DeviceAddress` @25, `Speed` @23, descriptor @4.

Occupied ports (raw capture, `ret` in brackets):

```
hub 4&196F3462 port 4 [57]:
04 00 00 00 12 01 00 02 ef 00 00 40 c6 27 9a 65 00 01 01 02 03
01 01 01 00 01 00 02 00 00 00 01 00 00 00 07 05 83 02 40 00 00 00 00 00 00 07 05
    bcdUSB=0x0200  class=0xef  idVendor=0x27c6 idProduct=0x659a  bcdDevice=0x0100
    currentConfig=1  Speed=1  DeviceIsHub=0  Address=1  pipes=2  status=1 (connected)

hub 4&196F3462 port 5 [57]:
05 00 00 00 12 01 01 02 ef 02 01 40 4f 17 2b 18 11 10 03 01 02
01 01 02 00 02 00 02 00 00 00 01 00 00 00 07 05 83 03 20 00 06 00 00 00 00 07 05
    bcdUSB=0x0201  class=0xef  idVendor=0x174f idProduct=0x182b  bcdDevice=0x1118
    currentConfig=1  Speed=2  DeviceIsHub=0  Address=2  pipes=3  status=1

hub 4&196F3462 port 8 [46]:
08 00 00 00 12 01 10 01 00 00 00 08 6d 04 42 c5 08 03 01 02 00
01 01 01 00 03 00 01 00 00 00 01 00 00 00 07 05 82 03 08 00 04 00 00 00 00 00 00
    bcdUSB=0x0110  class=0x00  idVendor=0x046d idProduct=0xc542  bcdDevice=0x0308
    currentConfig=1  Speed=1  DeviceIsHub=0  Address=3  pipes=1  status=1
```

All other 14 ports (hub 1: 1-5, hub 2: 1-3, 6, 7, 9-12) returned an all-zero
`USB_NODE_CONNECTION_INFORMATION_EX` (`ConnectionStatus = 0`), and descriptor/DRIVERKEY ioctls
failed with `ERROR_NO_SUCH_DEVICE` (433) — that is the empty-port signature, not an error path.

### 4.3 Device identity join (PnP)

`DEVPKEY_Device_Driver` of the port's child devnode equals the `DRIVERKEY_NAME` the hub returned
for that port, which makes the join exact rather than address-guessed:

| port | PnP instance id | driver key | bus-reported description | location | class |
|---|---|---|---|---|---|
| 4 | `USB\VID_27C6&PID_659A\UID2E0F0FFB_XXXX_MOC_B0` | `{53d29ef7-377c-4d14-864b-eb3a85769359}\0000` | `Goodix USB2.0 MISC` | `Port_#0004.Hub_#0002` | Biometric |
| 5 | `USB\VID_174F&PID_182B\0001` | `{36fc9e60-c465-11cf-8056-444553540000}\0007` | `Integrated Camera` | `Port_#0005.Hub_#0002` | USB |
| 8 | `USB\VID_046D&PID_C542\5&33D01247&0&8` | `{745a17a0-74d3-11d0-b6fe-00a0c90f57da}\0021` | `Wireless Receiver` | `Port_#0008.Hub_#0002` | HIDClass |

The same three devices are the only `{a5dcbf10-6530-11d2-901f-00c04fb951ed}` (USB device)
interfaces on the machine, so the child-devnode join and the device-interface view agree.
`DEVPKEY_Device_Address` also equals the port number for these direct root-hub children, but the
implementation joins on driver key/parent, not on address, because a hub-attached hub reuses
addresses per hub.

### 4.4 Active configuration descriptor (`bMaxPower`)

`IOCTL_USB_GET_DESCRIPTOR_FROM_NODE_CONNECTION`, `wValue = 0x0200`, `wLength = 255`; response is
`USB_DESCRIPTOR_REQUEST` (**12** bytes: `ConnectionIndex` + 8-byte setup packet) followed by the
descriptor, `ret = 12 + wTotalLength`.

```
port 4 [44]: ... 09 02 20 00 01 01 03 a0 32  -> wTotalLength 32, bmAttributes 0xa0, bMaxPower 50
port 5 [1024]: ... 09 02 9d 04 05 01 04 80 fa -> wTotalLength 1181, bmAttributes 0x80, bMaxPower 250
port 8 [46]: ... 09 02 22 00 01 01 00 a0 19   -> wTotalLength 34, bmAttributes 0xa0, bMaxPower 25
```

`bMaxPower` units are the USB 2.0 ones (2 mA) for these devices because `bcdUSB < 0x0300`:
100 mA / 500 mA / 50 mA. Nothing here is a PD contract, so this is reported as
`max_power_ma` ("as declared by the device's active configuration"), never as port capability.

## 5. `..._INFORMATION_EX_V2` is not usable on this build

`IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX_V2` (= `0x22045C`, kernel `USB_GET_NODE_CONNECTION_INFORMATION_EX_V2`)
returned `FALSE`, `GetLastError() = 87` (`ERROR_INVALID_PARAMETER`) for:

| port | `Length` field | in/out buffer | result |
|---|---|---|---|
| 5 | 16 | 16 / 16 | 87 |
| 5 | 16 | 16 / 4096 | 87 |
| 5 | 16 | 4096 / 4096 | 87 |
| 5 | 20 | 20 / 20 | 87 |
| 5 | 0 | 16 / 16 | 87 |
| 4, 1 (empty), hub-1 port 1 | 16 | 16 / 16 | 87 |

The struct layout used was `{ConnectionIndex, Length, SupportedUsbProtocols, Flags}` (16 bytes,
the Win32 metadata/windows-sys order). Because this IOCTL never answers, **`usb ports` does not
use it**: it does not claim SuperSpeedPlus, USB4 attachment speed, or per-port protocol support
from that surface. USB4 is reported only as a host router that exists, and its ports are reported
as `unavailable` rather than synthesised from the xHCI root hubs.

`IOCTL_USB_GET_HUB_CAPABILITIES_EX` does answer (`ret = 4`, value `0x00000012`) and
`IOCTL_USB_GET_HUB_CAPABILITIES` answers `0x00000000`; the capability bit union is not decoded by
`usb ports` because the meaning of the surviving bits could not be proven against a second
observation (both root hubs report the identical word, and there is no non-root hub on the unit
to compare). Raw hub facts that *are* decoded come from the hub descriptor (§4.1).

## 6. Power delivery and BC1.2: what exists and what is readable

### 6.1 UCSI (PD) stack exists, but has no user-mode connector interface

- `ACPI\USBC000\0` is a present, problem-free UCM device driven by `UcmUcsiAcpiClient`
  (BIOS device `\_SB.UBTC`, firmware 1.2). This is the platform's PD/PPM (UCSI) stack.
- SetupAPI enumeration of **device interfaces** under the UCM class GUID
  `{e6f1aa1c-7f3b-4473-b2e8-c97d8ac71d53}` returns **0** interfaces on this build: there is no
  openable user-mode connector object to issue a `GET_CONNECTOR_STATUS`-class request against.
  `UcmUcsiCx`'s request surface is a kernel-client (UcmCx) API, not a Win32 one.
- Consequence: PD operational mode, PDO list, negotiated voltage/current and cable e-marker data
  are **not readable** here, so no PD wattage is reported for any port.

### 6.2 No WMI class exposes connector/PD state

Namespace sweep (`Get-CimClass` across `root/wmi`, `root/cimv2`, and every `root/<ns>`):

- `root/wmi`: no `Ucm*`, `Usb4*`, `TypeC*`, `Charger*`, `Connector*` or `Pdo*` class. USB/power
  classes present are `Battery*`, `MSPower`, `UsbStorGuid`, `LENOVO_GAMEZONE_POWER_CHARGE_MODE_EVENT`,
  `LENOVO_REPORT_POWER_CONSUMPTION_CHANGE_EVENT`.
- `root/cimv2`: only `Win32_USBController`, `Win32_USBControllerDevice`, `Win32_USBHub`,
  `CIM_USBController/Hub/Device`, `Win32_PortConnector`, `CIM_PhysicalConnector`.

### 6.3 `Win32_PortConnector` is SMBIOS boilerplate — rejected as a port source

It returns template rows on this unit, not the chassis' real connectors:
`Keyboard` (`J1A1`, type 13), `Mouse` (`J1A1`, 14), `TV OUT` (`J2A1`, 28), `CRT` (`J2A2`, 28),
`COM 1` (`J2A2`, 9), then repeated `USB` (`J3A1`, `J5A1`, … type 16). There is no USB-C,
Thunderbolt or PD designator, the internal reference designators repeat, and nothing binds a row
to a controller port. Using it would fabricate a port list, so it is not a source.

### 6.4 EnergyDrv only carries always-on charging switches

The RE-proven `\\.\EnergyDrv` USB contract (`0x831020e8`, docs/interfaces.md §11.2) returns the
AC/battery always-on enable word (bits 6/7/14/15). It has no per-port, per-connector or PDO
payload; it is the *charging switch* channel, already exposed by `usb always-on` /
`usb charge-on-battery`, and is not used as a PD source.

### 6.5 Reported resolution

`power_delivery.pd_controller` = the UCSI device (instance id, driver, firmware).
`power_delivery.per_port_unavailable` = a fixed, measured sentence: UCSI present, zero user-mode
connector interfaces, no WMI connector class, hub port IOCTLs carry no charger type.
Each port's `power_delivery` is `null`. A future channel that really reads a port's contract can
fill `UsbPortPowerDelivery` without changing the CLI shape.

## 7. What `vantage usb ports` reports

Per controller (xHCI by PCI instance id, or the USB4 host router):

- controller instance id, description, driver service;
- each hub under it: instance id, `port_count` from the hub descriptor, `bus_powered`;
- each port: number, `connected`, `speed` (`low` / `full` / `high` / `super_speed`, or the raw
  byte when the driver returns a value outside `USB_DEVICE_SPEED` — never coerced), attached
  device (`instance_id`, `description`, `vendor_id`, `product_id`, `usb_revision` from `bcdUSB`,
  `max_power_ma` from the active configuration), and `power_delivery` (null on this build);
- the USB4 host router with `ports_unavailable` set to the measured reason (no user-mode port
  enumeration on the USB4 device-router stack);
- a hub whose interface cannot be opened or whose buffers do not decode is reported on its
  controller as `ports_unavailable` with the exact channel error, so one unreadable hub cannot
  hide the other hubs' real port data;
- the machine-level `power_delivery` block of §6.5.

Speed source per port is the hub driver's `USB_DEVICE_SPEED` byte only. `USB_DEVICE_SPEED` has
`UsbLowSpeed=0`, `UsbFullSpeed=1`, `UsbHighSpeed=2`, `UsbSuperSpeed=3`; anything else is printed
as the raw value.

Explicitly **not** claimed: PD wattage, BC1.2/DCP/CDP support, per-port SuperSpeedPlus/USB4
capability, cable or partner identity, physical connector names, or a USB4 port count.

## 8. Authority and lifecycle

- Read-only: no IOCTL in this path is a set-class request; `usb ports` never writes, never cycles
  a port, never touches `USB_HUB_CYCLE_PORT`. Hub handles are opened with desired access 0.
- Command axes: `read_only(StateLayer::Runtime)`; `DispatchSupport::Implemented`.
- Capability row `usb.ports` is recorded by a cheap probe (hub-interface + controller presence
  count, no per-port enumeration, no device tree walk) and is `Available` only when at least one
  hub interface answers during capability discovery.
- Platform: Windows HAL only. The Linux HAL exposes no USB port inventory, so `usb ports` there
  returns `Unsupported` for `usb.ports` rather than an empty inventory.

## 9. Unrun acceptance (parent-owned)

- `cargo check -p lctrl-hal-win --target x86_64-pc-windows-msvc` and the target build.
- `vantage usb ports` on 21VG with the dock/ports in the state captured here: expect
  `E47D` → 12-port hub with ports 4 (Goodix, `full`), 5 (camera, `high`), 8 (receiver, `full`)
  and nine empty ports; `E431` → 5-port hub, all empty; `E434` → USB4 host router with
  `ports_unavailable`; `power_delivery.per_port_unavailable` present.
- Negative control: detach the receiver, re-run, and expect port 8 to become `connected=false`
  with no device and no speed (empty-port signature), not a stale device.
- `vantage capability` must show `usb.ports` as a probed row (`available` on this target).
