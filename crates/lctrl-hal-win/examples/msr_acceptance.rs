//! Guided MSR acceptance run for C25 (owner-supervised).
//!
//! Default mode is read-only: handshake + baseline of every selector.
//! The write path is a provably state-preserving no-op: it re-writes the
//! value it just read and verifies the readback, then re-reads to confirm
//! nothing drifted. Run with `--write` only with the owner physically
//! present; the machine state cannot change because the written value
//! equals the freshly observed baseline.

use lctrl_hal_win::{MsrSelector, PLATFORM_ID_21VG, WinMsrDev, WIN_MSR_TRANSPORT};

fn main() {
    let write = std::env::args().any(|arg| arg == "--write");
    let client = WinMsrDev::new(&WIN_MSR_TRANSPORT);

    println!("mode: {}", if write { "WRITE (no-op, owner present)" } else { "READ-ONLY" });
    client
        .notify_platform_info(PLATFORM_ID_21VG, 0)
        .expect("NotifyPlatformInfo(0x36) handshake");

    let mut baseline = Vec::new();
    for selector in MsrSelector::all() {
        let value = client
            .read_selector(selector)
            .unwrap_or_else(|error| panic!("baseline read selector {}: {error}", selector.raw()));
        println!("baseline selector {} (MSR 0x{:03X}): 0x{value:016X}", selector.raw(), selector.msr_address());
        baseline.push(value);
    }

    if !write {
        println!("read-only acceptance complete; re-run with --write (owner present) for the write path");
        return;
    }

    // Write-path acceptance: same-value write on the platform power limit
    // selector (5 / MSR 0x65C). The value written equals the baseline read
    // microseconds earlier, so machine state is provably unchanged; the run
    // still exercises WRMSR through the driver and verifies readback.
    let target = MsrSelector::PLATFORM_POWER_LIMIT;
    let before = client
        .read_selector(target)
        .expect("pre-write readback of selector 5");
    client
        .write_selector(target, before)
        .expect("same-value write to selector 5 (MSR 0x65C)");
    let after = client
        .read_selector(target)
        .expect("post-write readback of selector 5");
    assert_eq!(before, after, "readback drifted across the same-value write");

    // HWP request selector (4 / MSR 0x774): same-value write exercises the
    // per-core HWP register without altering any field.
    let target = MsrSelector::HWP_REQUEST;
    let before = client
        .read_selector(target)
        .expect("pre-write readback of selector 4");
    client
        .write_selector(target, before)
        .expect("same-value write to selector 4 (MSR 0x774)");
    let after = client
        .read_selector(target)
        .expect("post-write readback of selector 4");
    assert_eq!(before, after, "HWP readback drifted across the same-value write");

    // Full-table re-read proves no selector drifted as a side effect.
    for (selector, &expected) in MsrSelector::all().iter().zip(baseline.iter()) {
        let now = client
            .read_selector(*selector)
            .expect("final sweep read");
        assert_eq!(now, expected, "selector {} drifted", selector.raw());
    }
    println!("write-path acceptance complete: same-value writes verified by readback, no drift");
}
