# 21VG GBMD selector observation — 2026-11-02

Target: TB14P-EZRA, existing EnergyDrv device. This is a driver-state observation, **not physical charging-threshold acceptance or official-UI acceptance**.

## Native observation

A target-native Rust consumer used the existing `NativeIoctl` transport. Each read used selector `0xff` in a separate transport call after a 300 ms wait. Writes returned zero output bytes.

| Step | Selector | GBMD readback |
| --- | --- | --- |
| Baseline | read `0xff` | `0x00860004` |
| Enable candidate storage mode | `0x03` | `0x00860022` |
| Disable candidate storage mode | `0x05` | `0x00860000` |
| Restore original rapid mode through production CLI | `battery charge-mode rapid --yes` | `0x00860004` |
| Disable candidate storage mode from rapid baseline | `0x05` | `0x00860004` |

The original rapid baseline was restored and separately read back using `charge_domain_probe.exe`.

The first probe incorrectly treated `0x05` alone as full baseline restoration. It correctly reported `restored=false`; the production CLI then restored rapid. A repeated pair observation had the same incomplete restoration and was also followed by explicit rapid restoration. Neither pair is a successful automatic-restore receipt.

## Supported conclusions

- `0x03` set bits 5 and 1 and cleared bit 2 on this target.
- `0x05` cleared the storage state after that transition; from an existing rapid baseline it was a no-op.
- The existing decoder maps bit 5 to conservation and bit 2 to rapid.
- Enabling storage does not preserve a previous rapid setting; restoration must restore the original semantic mode, not merely send the opposite selector.

## Limits and rejected evidence

- No independent official Vantage UI/CDP observation was captured.
- No physical 75–80% charge cutoff, persistence over reboot, or powered-off behavior was observed.
- The readback uses the same driver as the write, in separate calls. This is not an independent hardware sensor or a second implementation of the firmware contract.
- Earlier PowerShell raw-IOCTL attempts suffered signed-hex conversion errors; later attempts used an incorrect decimal IOCTL constant and did not establish device effects. Their zero buffers are not valid state receipts.
- Mode naming and admission must additionally follow the exact installed vendor implementation; a bit transition alone does not establish the full Storage80 contract.
