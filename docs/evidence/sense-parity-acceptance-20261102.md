# SmartSense vendor-parity write acceptance — TB14P-EZRA, 2026-11-02

Source: current uncommitted parity implementation in `lctrl-hal-win/src/sense.rs` (WaitNamedPipeW 3000 ms; SECURITY_SQOS_PRESENT | SECURITY_ANONYMOUS; nonzero accepted; exact 8-byte frame), built natively on target.

Baseline and all subsequent independent registry reads:

```
PresentLeave=1 Approach=1 Video=1 Capability=1 SensorType=2 FacialFeatureRegistered=1
```

Dry-run `vantage --dry-run sense pause-video off --json` exited 0 and returned previous.pause_video=true, requested.pause_video=false; registry remained Video=1.

Commit `vantage --yes --auto-restore sense pause-video off --json` exited 6:

```
firmware_rejected: SmartSense pipe rejected command 0x000d0101 arg 2
```

Independent registry remained Video=1. The service returned zero after exact vendor pipe-security and connect-timeout parity. Therefore the correction does not unlock the feature. No accepted write occurred, no partial state was left, and no restoration was needed.

The newer `HumanPresenceDetectionEndpoint` RPC capability and settings contract remain the next recovery path; direct registry writes are not a substitute for hardware application and were not attempted.
