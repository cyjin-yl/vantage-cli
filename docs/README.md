# Documentation

## For users

- [README](../README.md) — install, usage, design philosophy
- [CAPABILITY](CAPABILITY.md) — per-command support matrix with reasons
- [IMPLEMENTATION](IMPLEMENTATION.md) — how each feature works (Windows and Linux APIs)

## Internal reference

These documents were written during development and contain detailed
specifications for contributors.  They are not required reading for
users.

| Doc | Contents |
|-----|----------|
| [00 Project charter](00-project-charter.md) | Goals, non-goals, safety boundaries |
| [01 HAL interfaces](01-hal-interfaces.md) | Trait definitions and platform abstraction |
| [02 Power & battery](02-power-battery.md) | Charge mode, thresholds, night charge, GBMD/SBMC |
| [03 Thermal & perf](03-thermal-performance.md) | Thermal zones, fan control, ITS, ESIF |
| [04 Peripherals](04-peripherals.md) | Keyboard, touchpad, panel, audio, sensors, privacy |
| [05 BIOS settings](05-bios-settings.md) | WMI BIOS staging, password, defaults |
| [06 MagicBay](06-magicbay.md) | USB/ACPI accessory detection, LTE, camera, display |
| [07 PantherLake tuning](07-pantherlake-tuning.md) | PL1/PL2/tau, EPP, powercap, MSR |
| [08 Architecture](08-architecture.md) | Crate structure, build, IPC |
| [09 Linux backend](09-linux-backend.md) | sysfs paths, ideapad_acpi, DRM, powercap |
| [10 Config schema](10-config-schema.md) | Profile TOML, managed settings, preferences |
| [11 Updates & diagnostics](11-sysupdate-diagnostics.md) | UDC catalog, LDE scan, local install |
| [A WMI reference](A-wmi-reference.md) | WMI class/method inventory per namespace |
| [B Evidence](B-evidence.md) | Target machine observations and probing receipts |
| [C Acceptance](C-acceptance.md) | Per-feature hardware acceptance criteria |
| [interfaces](interfaces.md) | Windows service/driver channel specifications |

## Evidence

The `evidence/` directory contains per-feature investigation receipts
from target-machine probing.  These are primary sources — they record
what the firmware actually returned, not what we expected.
