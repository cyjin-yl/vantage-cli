# Privacy camera + OSD — channel recon — 2026-11-05

## Camera privacy
Not a physical shutter GPIO — 21VG camera privacy is **PnP devnode
enable/disable** of `Integrated Camera` (`USB\VID_174F&PID_182B&MI_00`), plus
the Windows media-capture **occlusion API**. Owner addins:
- `LenovoMachineFixSystemAddin\...\CameraModule.dll` — symbols
  `CameraShutterMonitor::{InitMediaCapture,IsShutterOff,HandleOcclusionState,
  OnCaptureDeviceExclusiveControl}`, `CameraUsedApp::{Add,TryGet}ShutterSuppressedTrigger`,
  `DriverHelper::EnableByPropertyChange`, `StartDevice: CM_Enable_DevNode
  Success for device '%s'`. → the kill switch is `CM_Enable_DevNode` /
  `DriverHelper::EnableByPropertyChange` on the camera devnode.
- `SmartDisplayAddin\...\x64\SECameraPlugin.dll` — smart-display camera assist.
- Related devnodes: `Integrated IR Camera` (MI_02), `Lenovo Camera Group`,
  `Windows Studio Effects Camera`, `Camera DFU Device` (MI_04).

→ `vantage privacy camera` can be implemented as PnP enable/disable of the
`Integrated Camera` devnode via `pnputil`/SetupAPI — same device-control
channel class as touchpad. Occlusion read via `CameraOcclusionInfo`
(media-capture API), not a WMI class (no `Lenovo_SetCameraStatus`/`Lenovo_CameraStatus`
under `root\wmi`).

## OSD (on-screen display)
Owner: `SmartInteractAddin\1.0.9.3\OSDControl.exe` — a Win32 app
(`COSDControlApp`, `COSDShowDlg`) that shows FN/OSD overlays via window
messages (`AIRGESTURE_OSD_FORCELYCLOSE`, `AirGestureOSDShow`). Also related:
`FnHotkeyUtility.exe` + `LenovoUtilityService.exe` (FnAndFunctionKeys driver
store package) drive the FN hotkey OSD.

→ OSD enable/disable is a UI-app + Fn-driver toggle, not a single registry
flag found yet; the launch surface is `OSDControl.exe` + the
FnAndFunctionKeys package. OSD_ENABLE/DISABLE implementation needs the toggle
target pinned (a service state or a `OSDControl`-managed setting) — left as
recon-complete, implement after the toggle channel is confirmed.

Target scratch: `privacy.txt`, `osd.txt` under `C:\Users\ezra\`.
