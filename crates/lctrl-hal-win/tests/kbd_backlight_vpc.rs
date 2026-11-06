//! Contract tests for the ACPI `KBLC` keyboard-backlight channel.
//!
//! The transport is scripted: the shape proven from `AcpiVpc.sys` is a 4-byte
//! argument in and a 4-byte result out on `0x83102144`, with the argument
//! multiplexing query/get/set in its low nibble.

use std::collections::VecDeque;
use std::sync::Arc;

use lctrl_core::{ApplyMode, BacklightState, LctrlError, LightingEffect};
use lctrl_hal::KeyboardControl;
use lctrl_hal_win::{
    IOCTL_VPC_KBLC, IoctlTransport, KblcLayout, WindowsKeyboardBacklight, WindowsKeyboardSources,
    kblc_argument_writes, probe_vpc_keyboard,
};
use parking_lot::Mutex;

/// Scripted `IoctlTransport`: records every call, answers from a reply queue.
#[derive(Default)]
struct FakeIoctl {
    replies: Mutex<VecDeque<lctrl_core::Result<u32>>>,
    calls: Mutex<Vec<(u32, u32)>>,
}

impl FakeIoctl {
    fn new(replies: impl IntoIterator<Item = lctrl_core::Result<u32>>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().collect()),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<(u32, u32)> {
        self.calls.lock().clone()
    }

    fn set_arguments(&self) -> Vec<u32> {
        self.calls
            .lock()
            .iter()
            .filter_map(|(code, argument)| {
                (*code == IOCTL_VPC_KBLC && argument & 0x0f == 0x3).then_some(*argument)
            })
            .collect()
    }
}

impl IoctlTransport for FakeIoctl {
    fn call(
        &self,
        code: u32,
        input: &[u8],
        output_len: usize,
    ) -> lctrl_core::Result<Vec<u8>> {
        let [a, b, c, d] =
            <[u8; 4]>::try_from(input).expect("the KBLC argument is four bytes");
        let argument = u32::from_le_bytes([a, b, c, d]);
        self.calls.lock().push((code, argument));
        let reply = self.replies.lock().pop_front().ok_or_else(|| {
            LctrlError::ChannelUnavailable {
                channel: "scripted KBLC reply exhausted".into(),
            }
        })?;
        assert_eq!(output_len, 4, "KBLC always answers one 32-bit word");
        reply.map(|word| word.to_le_bytes().to_vec())
    }
}

fn query(word: u32) -> lctrl_core::Result<u32> {
    Ok(word)
}

/// `KBLC` get reply: the firmware answers `KBGS | 1`.
fn get(state: u8) -> lctrl_core::Result<u32> {
    Ok(u32::from(state) << 1 | 1)
}

fn controller(
    replies: impl IntoIterator<Item = lctrl_core::Result<u32>>,
) -> WindowsKeyboardBacklight<FakeIoctl> {
    WindowsKeyboardBacklight::new(FakeIoctl::new(replies))
}

#[test]
fn layout_reports_the_published_keyboard_group() {
    assert_eq!(KblcLayout::from_query(0x7), Some(KblcLayout::TriStateAuto));
    assert_eq!(KblcLayout::from_query(0x5), Some(KblcLayout::TriState));
    assert_eq!(KblcLayout::from_query(0x3), None);
    assert_eq!(KblcLayout::from_query(0x0), None);

    assert_eq!(KblcLayout::TriState.group(), 2);
    assert_eq!(KblcLayout::TriState.max_state(), 2);
    assert_eq!(KblcLayout::TriState.auto_state(), None);
    assert_eq!(KblcLayout::TriStateAuto.group(), 3);
    assert_eq!(KblcLayout::TriStateAuto.max_state(), 3);
    assert_eq!(KblcLayout::TriStateAuto.auto_state(), Some(3));
}

#[test]
fn probe_rejects_a_keyboard_group_this_command_set_does_not_drive() {
    let error = probe_vpc_keyboard(&FakeIoctl::new([query(0x3)])).unwrap_err();
    assert!(
        matches!(&error, LctrlError::Unsupported { feature } if feature.contains("0x3")),
        "unexpected error: {error:?}"
    );
}

#[test]
fn only_the_set_command_requires_write_access() {
    assert_eq!(kblc_argument_writes(0x1), Some(false), "query");
    assert_eq!(kblc_argument_writes(0x32), Some(false), "get, group 3");
    assert_eq!(kblc_argument_writes(0x22), Some(false), "get, group 2");
    assert_eq!(kblc_argument_writes(0x33), Some(true), "set off");
    assert_eq!(kblc_argument_writes(0x20033), Some(true), "set high");
    assert_eq!(kblc_argument_writes(0x4), None, "undefined command nibble");
    assert_eq!(kblc_argument_writes(0x0), None, "undefined command nibble");
}

#[test]
fn state_reads_use_the_query_group_and_decode_the_state_bits() {
    for state in 0..=3u8 {
        let service = controller([query(0x7), get(state)]);
        let actual = service.backlight_state().unwrap();
        assert_eq!(
            actual,
            BacklightState::new(state, 3, LightingEffect::Static).unwrap()
        );
        assert_eq!(
            service.ioctl().calls(),
            vec![(IOCTL_VPC_KBLC, 0x1), (IOCTL_VPC_KBLC, 0x32)],
            "query then get with group 3 in bits 7..4"
        );
    }
}

#[test]
fn tristate_get_uses_group_two() {
    let service = controller([query(0x5), get(1)]);
    let actual = service.backlight_state().unwrap();
    assert_eq!(actual.level, 1);
    assert_eq!(actual.max_level, 2);
    assert_eq!(service.ioctl().calls()[1], (IOCTL_VPC_KBLC, 0x22));
}

#[test]
fn no_status_answer_is_not_read_as_off() {
    // A group mismatch answers zero, not a state word; decoding it as "off"
    // would report a backlight state the firmware never gave.
    let error = controller([query(0x7), Ok(0)])
        .backlight_state()
        .unwrap_err();
    assert!(
        matches!(&error, LctrlError::InvalidArgument { detail } if detail.contains("no status")),
        "unexpected error: {error:?}"
    );
}

#[test]
fn state_outside_the_advertised_group_is_rejected() {
    let error = controller([query(0x7), get(4)])
        .backlight_state()
        .unwrap_err();
    assert!(
        matches!(&error, LctrlError::InvalidArgument { detail } if detail.contains("0..=3")),
        "unexpected error: {error:?}"
    );
}

#[test]
fn short_reply_is_a_firmware_rejection() {
    struct ShortReply;

    impl IoctlTransport for ShortReply {
        fn call(
            &self,
            _code: u32,
            _input: &[u8],
            _output_len: usize,
        ) -> lctrl_core::Result<Vec<u8>> {
            Ok(vec![0x07])
        }
    }

    let error = probe_vpc_keyboard(&ShortReply).unwrap_err();
    assert!(
        matches!(&error, LctrlError::FirmwareRejected { detail } if detail.contains("1 bytes")),
        "unexpected error: {error:?}"
    );
}

#[test]
fn dry_run_reads_but_never_writes() {
    let service = controller([query(0x7), get(0)]);
    let report = service
        .set_backlight(2, LightingEffect::Static, ApplyMode::DryRun)
        .unwrap();

    assert_eq!(report.previous().level, 0);
    assert_eq!(report.requested().level, 2);
    assert!(report.actual().is_none());
    assert_eq!(service.ioctl().calls().len(), 2, "query + get only");
    assert!(service.ioctl().set_arguments().is_empty());
}

#[test]
fn commit_encodes_the_set_argument_and_verifies_with_a_get() {
    let service = controller([query(0x7), get(0), Ok(0), get(2)]);
    let report = service
        .set_backlight(2, LightingEffect::Static, ApplyMode::Commit)
        .unwrap();

    assert_eq!(report.previous().level, 0);
    assert_eq!(report.requested().level, 2);
    assert_eq!(report.actual().map(|actual| actual.level), Some(2));
    assert_eq!(
        service.ioctl().set_arguments(),
        vec![0x20033],
        "state 2 << 16 | group 3 << 4 | set"
    );
}

#[test]
fn auto_state_is_settable_only_when_the_firmware_publishes_it() {
    let service = controller([query(0x7), get(1), Ok(0), get(3)]);
    let report = service
        .set_backlight(3, LightingEffect::Static, ApplyMode::Commit)
        .unwrap();
    assert_eq!(report.requested().level, 3);
    assert_eq!(service.ioctl().set_arguments(), vec![0x30033]);

    let tristate = controller([query(0x5), get(1)]);
    let error = tristate
        .set_backlight(3, LightingEffect::Static, ApplyMode::DryRun)
        .unwrap_err();
    let detail = match &error {
        LctrlError::InvalidArgument { detail } => detail,
        other => panic!("unexpected error: {other:?}"),
    };
    assert!(
        detail.contains("exceeds max 2"),
        "unexpected detail: {detail}"
    );
    assert!(tristate.ioctl().set_arguments().is_empty());
}

#[test]
fn lighting_effects_are_refused_before_anything_is_written() {
    let service = controller([query(0x7)]);
    let error = service
        .set_backlight(1, LightingEffect::Breathing, ApplyMode::Commit)
        .unwrap_err();
    assert!(
        matches!(&error, LctrlError::Unsupported { feature } if feature == "kbd.backlight.effect"),
        "unexpected error: {error:?}"
    );
    assert_eq!(service.ioctl().calls(), vec![(IOCTL_VPC_KBLC, 0x1)]);
}

#[test]
fn failed_readback_restores_the_previous_state() {
    let mut replies = vec![query(0x7), get(0)];
    replies.push(Ok(0));
    // Ten verification reads keep answering the old state, then the rollback
    // re-read confirms the restore.
    replies.extend((0..10).map(|_| get(0)));
    replies.push(Ok(0));
    replies.push(get(0));
    let service = controller(replies);

    let error = service
        .set_backlight(2, LightingEffect::Static, ApplyMode::Commit)
        .unwrap_err();

    assert!(
        matches!(&error, LctrlError::VerifyMismatch { .. }),
        "unexpected error: {error:?}"
    );
    assert_eq!(
        service.ioctl().set_arguments(),
        vec![0x20033, 0x33],
        "the failed write is followed by one restore of state 0"
    );
}

#[test]
fn rollback_failure_is_reported_as_firmware_rejection() {
    let mut replies = vec![query(0x7), get(0), Ok(0)];
    replies.extend((0..10).map(|_| get(0)));
    // Rollback write succeeds but its readback never matches again.
    replies.push(Ok(0));
    replies.extend((0..10).map(|_| get(1)));
    let service = controller(replies);

    let error = service
        .set_backlight(2, LightingEffect::Static, ApplyMode::Commit)
        .unwrap_err();

    let detail = match &error {
        LctrlError::FirmwareRejected { detail } => detail,
        other => panic!("unexpected error: {other:?}"),
    };
    assert!(
        detail.contains("rollback also failed"),
        "unexpected detail: {detail}"
    );
}

/// Scripted `KeyboardControl` for the routing tests. Cloning shares the call
/// counter so a moved copy can still be inspected.
#[derive(Clone)]
struct ScriptedKeyboard {
    outcome: ScriptedOutcome,
    calls: Arc<Mutex<usize>>,
}

#[derive(Clone, Copy)]
enum ScriptedOutcome {
    State(BacklightState),
    Unsupported,
    ChannelUnavailable,
    InvalidArgument,
}

impl ScriptedKeyboard {
    fn answering(level: u8) -> Self {
        Self {
            outcome: ScriptedOutcome::State(
                BacklightState::new(level, 2, LightingEffect::Static).unwrap(),
            ),
            calls: Arc::new(Mutex::new(0)),
        }
    }

    fn failing(outcome: ScriptedOutcome) -> Self {
        Self {
            outcome,
            calls: Arc::new(Mutex::new(0)),
        }
    }

    fn calls(&self) -> usize {
        *self.calls.lock()
    }
}

impl KeyboardControl for ScriptedKeyboard {
    fn backlight_state(&self) -> lctrl_core::Result<BacklightState> {
        *self.calls.lock() += 1;
        match self.outcome {
            ScriptedOutcome::State(state) => Ok(state),
            ScriptedOutcome::Unsupported => Err(LctrlError::Unsupported {
                feature: "kbd.backlight".into(),
            }),
            ScriptedOutcome::ChannelUnavailable => Err(LctrlError::ChannelUnavailable {
                channel: "EnergyDrv KBLC".into(),
            }),
            ScriptedOutcome::InvalidArgument => Err(LctrlError::InvalidArgument {
                detail: "ACPI KBLC returned no status for keyboard group 3".into(),
            }),
        }
    }

    fn set_backlight(
        &self,
        level: u8,
        effect: LightingEffect,
        apply: ApplyMode,
    ) -> lctrl_core::Result<lctrl_core::ChangeReport<BacklightState>> {
        let previous = self.backlight_state()?;
        let requested = BacklightState::new(level, 3, effect)?;
        Ok(match apply {
            ApplyMode::DryRun => ChangeReport::dry_run(previous, requested),
            ApplyMode::Commit => ChangeReport::committed(previous, requested, requested),
        })
    }
}

#[test]
fn absent_primary_channel_falls_back_to_the_lighting_plane() {
    let primary = ScriptedKeyboard::failing(ScriptedOutcome::Unsupported);
    let secondary = ScriptedKeyboard::answering(1);
    let services = WindowsKeyboardSources::new(primary.clone(), secondary.clone());

    assert_eq!(services.backlight_state().unwrap().level, 1);
    assert_eq!(secondary.calls(), 1);
}

#[test]
fn device_errors_from_the_primary_channel_are_not_swallowed() {
    let primary = ScriptedKeyboard::failing(ScriptedOutcome::InvalidArgument);
    let secondary = ScriptedKeyboard::answering(1);
    let services = WindowsKeyboardSources::new(primary.clone(), secondary.clone());

    assert!(matches!(
        services.backlight_state(),
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert_eq!(secondary.calls(), 0);
}

#[test]
fn primary_error_survives_when_the_fallback_fails_too() {
    let primary = ScriptedKeyboard::failing(ScriptedOutcome::ChannelUnavailable);
    let secondary = ScriptedKeyboard::failing(ScriptedOutcome::Unsupported);
    let services = WindowsKeyboardSources::new(primary.clone(), secondary.clone());

    assert!(matches!(
        services.backlight_state(),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
    assert_eq!(secondary.calls(), 1);
}

#[test]
fn writes_route_the_same_way_as_reads() {
    let primary = ScriptedKeyboard::failing(ScriptedOutcome::Unsupported);
    let secondary = ScriptedKeyboard::answering(0);
    let services = WindowsKeyboardSources::new(primary.clone(), secondary.clone());

    let report = services
        .set_backlight(1, LightingEffect::Static, ApplyMode::DryRun)
        .unwrap();
    assert_eq!(report.requested().level, 1);
    assert_eq!(secondary.calls(), 1);
}
