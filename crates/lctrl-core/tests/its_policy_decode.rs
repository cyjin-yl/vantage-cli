//! The ITS `Policy_*` decode is a pure function of the registry values, so the
//! vendor's packing and clamping rules are pinned here instead of only being
//! exercised on the target machine.

use lctrl_core::{
    ItsApMinStdFan, ItsEppTurbo, ItsPlxSetMode, ItsSlotTable, ItsTurboTime, ItsTurboValue,
};

#[test]
fn plx_set_mode_splits_the_two_lanes_with_the_raw_value_kept() {
    let mode = ItsPlxSetMode::from_raw(0x0001_0001);
    assert_eq!(mode, ItsPlxSetMode { raw: 0x0001_0001, ac: 1, dc: 1 });

    // Low half is AC, high half is DC: 0x0002_0003 is AC=3, DC=2.
    let mixed = ItsPlxSetMode::from_raw(0x0002_0003);
    assert_eq!((mixed.ac, mixed.dc), (3, 2));
    assert_eq!(mixed.raw, 0x0002_0003);
}

#[test]
fn ap_min_std_fan_clamps_thresholds_the_way_the_dispatcher_does() {
    // Values above 100 are replaced by 50; 100 itself is kept.
    let clamped = ItsApMinStdFan::from_raws(1, 60, 150);
    assert!(clamped.enable);
    assert_eq!(clamped.gpu_threshold_pct, 60);
    assert_eq!(clamped.cpu_threshold_pct, 50);

    let boundary = ItsApMinStdFan::from_raws(0, 0, 100);
    assert!(!boundary.enable);
    assert_eq!(boundary.gpu_threshold_pct, 0);
    assert_eq!(boundary.cpu_threshold_pct, 100);

    assert_eq!(ItsApMinStdFan::clamp_threshold(101), 50);
}

#[test]
fn turbo_value_marks_the_default_epp_sentinel_without_hiding_the_raw_value() {
    let default_epp = ItsTurboValue::from_raw(0xff);
    assert!(default_epp.default_epp);
    assert_eq!(default_epp.raw, 0xff);

    let specific = ItsTurboValue::from_raw(20);
    assert!(!specific.default_epp);
    assert_eq!(specific.raw, 20);
}

#[test]
fn turbo_time_exposes_the_turbo_time_lane_and_the_seed() {
    let live = ItsTurboTime::from_raw(0x0808_0808);
    assert_eq!(live.turbo_time, 8);
    assert_eq!(live.byte0, 8);
    assert_eq!(live.raw, 0x0808_0808);

    // The bytes the dispatcher seeds before reading the registry.
    let seed = ItsTurboTime::from_raw(0x0503_0303);
    assert_eq!(
        (seed.byte0, seed.byte1, seed.turbo_time, seed.byte3),
        (3, 3, 3, 5)
    );
}

#[test]
fn epp_turbo_splits_nibble_turbo_time_and_the_apm_bit() {
    let decoded = ItsEppTurbo::from_raw(0x1508_0000);
    assert_eq!(decoded.mode_nibble, 5);
    assert_eq!(decoded.turbo_time, 8);
    assert!(decoded.not_turbo_to_apm);
    assert_eq!(decoded.raw, 0x1508_0000);

    let plain = ItsEppTurbo::from_raw(0x0009_0000);
    assert_eq!(plain.mode_nibble, 0);
    assert_eq!(plain.turbo_time, 9);
    assert!(!plain.not_turbo_to_apm);
}

#[test]
fn slot_table_flattens_the_number_run_into_pairs() {
    let table = ItsSlotTable::parse("1:2;3:4;5:6;7:8;9:10;11:12;", 12).unwrap();
    assert_eq!(table.capacity, 12);
    assert_eq!(table.slots, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    assert_eq!(
        table.pairs(),
        vec![(1, 2), (3, 4), (5, 6), (7, 8), (9, 10), (11, 12)]
    );
    assert_eq!(table.source, "1:2;3:4;5:6;7:8;9:10;11:12;");
}

#[test]
fn slot_table_accepts_the_published_cppc_default() {
    // The dispatcher's own default for `Policy_CPPC`, 8 pairs = 16 slots.
    let source = "50:80;50:72;50:72;40:60;40:50;40:50;30:50;30:50";
    let table = ItsSlotTable::parse(source, 16).unwrap();
    assert_eq!(table.slots.len(), 16);
    assert_eq!(table.pairs()[0], (50, 80));
    assert_eq!(table.pairs()[7], (30, 50));
}

#[test]
fn slot_table_refuses_a_value_that_does_not_fit_the_dispatcher_array() {
    // The published `Policy_PowerBucket` default holds seven pairs (14
    // numbers) while the dispatcher's array holds 12 slots. A short table
    // would be a guess, so the decode refuses it and reports the counts.
    let published = "0:15;10:30;25:45;38:50;40:70;60:80;70:100;";
    let error = ItsSlotTable::parse(published, 12).unwrap_err();
    assert!(
        error.contains("expected 12 slot value(s)") && error.contains("holds 14"),
        "unexpected detail: {error}"
    );

    let short = ItsSlotTable::parse("1:2;3:4;", 12).unwrap_err();
    assert!(short.contains("holds 4"), "unexpected detail: {short}");
}

#[test]
fn slot_table_ignores_non_digit_separators() {
    let table = ItsSlotTable::parse("  7 : 8 ; 9:10 ", 4).unwrap();
    assert_eq!(table.slots, vec![7, 8, 9, 10]);
}
