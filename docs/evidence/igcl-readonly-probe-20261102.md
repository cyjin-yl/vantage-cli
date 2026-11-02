# IGCL read-only native probe — TB14P-EZRA, 2026-11-02

Source: `crates/lctrl-hal-win/examples/igcl_probe.rs`, built natively on the target with the direct stable-MSVC toolchain. This isolated process performs no writes.

Observed output (the process was rerun with `Start-Process -Wait` and a bounded wait; its captured exit status was 0):

```
library: C:\Windows\System32\DriverStore\FileRepository\iigd_dch.inf_amd64_e55e939f4d7c3060\IntelControlLib.dll
display output handle: 0x2847019c668
pixtx: no pixel-transform blocks on this output (pipeline absent)
super-resolution caps: channel is unavailable: ctlGetSupportedVideoProcessingCapabilities(SUPER_RESOLUTION) failed (result 0x4000000b)
super-resolution flag: feature is unsupported: panel.super-resolution.state-query.rejected
```

Interpretation: the selected display output has no enumerated pixtx blocks. The super-resolution capability query returns `0x4000000b` (invalid argument) on this driver; the 92-byte get/set payload correction does not make the feature available. This is not a proof that another output or future driver lacks those paths. Eye-care and super-resolution writes remain unsupported/closed; do not infer hardware acceptance from ABI layout tests.

An initial SSH invocation timed out at 120 seconds. A subsequent process inspection found no `igcl_probe.exe`, so the timeout did not establish a live hang. The bounded rerun above produced the complete observation.
