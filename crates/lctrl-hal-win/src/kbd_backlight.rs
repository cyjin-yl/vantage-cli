//! Keyboard backlight through the Lenovo ACPI virtual power controller.
//!
//! `AcpiVpc.sys` 15.11.30.14 opens the `ACPI\VPC2004` device and re-exposes a
//! vendor IOCTL surface on `\\.\EnergyDrv`. Its dispatch (`0x140011bda`)
//! compares `0x83102144` and runs the handler at `0x140001fa4`, which builds a
//! 12-byte `IOCTL_ACPI_EVAL_METHOD_EX` request `{signature 0x49696541,
//! "KBLC", argument u32}` and validates the reply as an ACPI evaluation output
//! buffer (`0x426f6541`, integer argument, 20 bytes minimum) before copying the
//! result out. Net contract: 4-byte input argument, 4-byte output result.
//!
//! The ACPI `KBLC` method is the interface the Linux `ideapad-laptop` driver
//! uses for the same feature on VPC2004 machines, and its argument layout is
//! recovered there:
//!
//! ```text
//! 0x1                                  query  -> KBGC | 1  (5 tristate, 7 +auto)
//! (group << 4) | 0x2                   get    -> KBGS | 1  (state = (ret >> 1) & 7)
//! (state << 16) | (group << 4) | 0x3   set    -> state is re-read with get
//! ```
//!
//! `group = (query & ~1) >> 1`, so a tristate keyboard reports group 2 (states
//! off/low/high) and a tristate+auto keyboard reports group 3 (plus auto). The
//! firmware answers a `get` whose group field does not match with **zero**, not
//! with an error, so a zero reply is "no status", never "off".
//!
//! Recovered evidence: `docs/evidence/re-kbd-backlight-20261105.md`.

use std::time::Duration;

use lctrl_core::{ApplyMode, BacklightState, ChangeReport, LctrlError, LightingEffect, Result};
use lctrl_hal::{KeyboardControl, poll_readback};

use crate::{IoctlTransport, codec::decode_u32_exact};

/// Vendor IOCTL that evaluates the ACPI `KBLC` control method.
pub const IOCTL_VPC_KBLC: u32 = 0x8310_2144;

const COMMAND_QUERY: u32 = 0x1;
const COMMAND_GET: u32 = 0x2;
const COMMAND_SET: u32 = 0x3;
const COMMAND_MASK: u32 = 0x0f;

const GROUP_SHIFT: u32 = 4;
const STATE_SHIFT: u32 = 16;
/// `get` answers `KBGS | 1`; a group mismatch answers zero.
const NO_STATUS: u32 = 0;

/// Whether `KBLC` reaches a setter with this argument.
///
/// `Some(true)` is a write (needs write access to the device), `Some(false)` a
/// query/get, and `None` a command nibble this interface does not define — the
/// caller must refuse it rather than open the device for an unknown effect.
#[must_use]
pub const fn kblc_argument_writes(argument: u32) -> Option<bool> {
    match argument & COMMAND_MASK {
        COMMAND_QUERY | COMMAND_GET => Some(false),
        COMMAND_SET => Some(true),
        _ => None,
    }
}

/// Keyboard group the firmware publishes, i.e. the number of backlight states.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KblcLayout {
    /// Three states: off, low, high.
    TriState,
    /// Four states: off, low, high, auto (ambient-light controlled).
    TriStateAuto,
}

impl KblcLayout {
    /// Decode the `KBLC(0x1)` word. Any other value is a keyboard this command
    /// set does not drive, so it must not be forced into the state model.
    #[must_use]
    pub const fn from_query(word: u32) -> Option<Self> {
        match word {
            0x5 => Some(Self::TriState),
            0x7 => Some(Self::TriStateAuto),
            _ => None,
        }
    }

    /// Group selector bits the firmware publishes for this keyboard.
    #[must_use]
    pub const fn group(self) -> u32 {
        match self {
            Self::TriState => 2,
            Self::TriStateAuto => 3,
        }
    }

    /// Highest valid state index; states are `0 ..= max_state()`.
    #[must_use]
    pub const fn max_state(self) -> u8 {
        match self {
            Self::TriState => 2,
            Self::TriStateAuto => 3,
        }
    }

    /// State number of the ambient-light controlled mode, when published.
    #[must_use]
    pub const fn auto_state(self) -> Option<u8> {
        match self {
            Self::TriState => None,
            Self::TriStateAuto => Some(3),
        }
    }
}

/// Query the keyboard group without owning the transport, for capability
/// probes that share an already-bound `IoctlTransport`.
pub fn probe_vpc_keyboard<I>(ioctl: &I) -> Result<KblcLayout>
where
    I: IoctlTransport + ?Sized,
{
    let word = evaluate(ioctl, COMMAND_QUERY)?;
    KblcLayout::from_query(word).ok_or_else(|| LctrlError::Unsupported {
        feature: format!("kbd.backlight (ACPI KBLC reports keyboard group {word:#x})"),
    })
}

fn evaluate<I>(ioctl: &I, argument: u32) -> Result<u32>
where
    I: IoctlTransport + ?Sized,
{
    let output = ioctl.call(IOCTL_VPC_KBLC, &argument.to_le_bytes(), 4)?;
    decode_u32_exact(&output, "VPC KBLC")
}

/// Backlight controller backed by the ACPI `KBLC` method.
#[derive(Debug)]
pub struct WindowsKeyboardBacklight<I> {
    ioctl: I,
}

impl<I> WindowsKeyboardBacklight<I> {
    #[must_use]
    pub const fn new(ioctl: I) -> Self {
        Self { ioctl }
    }

    #[must_use]
    pub const fn ioctl(&self) -> &I {
        &self.ioctl
    }
}

impl<I> WindowsKeyboardBacklight<I>
where
    I: IoctlTransport,
{
    /// Keyboard group published by the firmware.
    pub fn layout(&self) -> Result<KblcLayout> {
        probe_vpc_keyboard(&self.ioctl)
    }

    fn read_state(&self, layout: KblcLayout) -> Result<BacklightState> {
        let word = evaluate(
            &self.ioctl,
            (layout.group() << GROUP_SHIFT) | COMMAND_GET,
        )?;
        if word == NO_STATUS {
            return Err(LctrlError::InvalidArgument {
                detail: format!(
                    "ACPI KBLC returned no status for keyboard group {}",
                    layout.group()
                ),
            });
        }
        let state = ((word >> 1) & 0x7) as u8;
        if state > layout.max_state() {
            return Err(LctrlError::InvalidArgument {
                detail: format!(
                    "ACPI KBLC reported state {state} outside the advertised 0..={} range",
                    layout.max_state()
                ),
            });
        }
        BacklightState::new(state, layout.max_state(), LightingEffect::Static)
    }

    /// Issue one `set`. The reply is not a status word on this interface, so
    /// the caller verifies the transition with [`Self::read_state`] instead of
    /// interpreting it.
    fn write_state(&self, layout: KblcLayout, state: u8) -> Result<()> {
        let argument =
            (u32::from(state) << STATE_SHIFT) | (layout.group() << GROUP_SHIFT) | COMMAND_SET;
        evaluate(&self.ioctl, argument).map(|_| ())
    }

    fn verify(&self, layout: KblcLayout, requested: &BacklightState) -> Result<BacklightState> {
        poll_readback(requested, 10, Duration::from_millis(50), || {
            self.read_state(layout)
        })
    }
}

impl<I> KeyboardControl for WindowsKeyboardBacklight<I>
where
    I: IoctlTransport,
{
    fn backlight_state(&self) -> Result<BacklightState> {
        let layout = self.layout()?;
        self.read_state(layout)
    }

    fn set_backlight(
        &self,
        level: u8,
        effect: LightingEffect,
        apply: ApplyMode,
    ) -> Result<ChangeReport<BacklightState>> {
        let layout = self.layout()?;
        if !matches!(effect, LightingEffect::Static) {
            return Err(LctrlError::Unsupported {
                feature: "kbd.backlight.effect".into(),
            });
        }
        let previous = self.read_state(layout)?;
        let requested = BacklightState::new(level, layout.max_state(), effect)?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        let result = self
            .write_state(layout, requested.level)
            .and_then(|()| self.verify(layout, &requested));
        match result {
            Ok(actual) => Ok(ChangeReport::committed(previous, requested, actual)),
            Err(error) => {
                let rollback = self
                    .write_state(layout, previous.level)
                    .and_then(|()| self.verify(layout, &previous).map(|_| ()));
                match rollback {
                    Ok(()) => Err(error),
                    Err(rollback) => Err(LctrlError::FirmwareRejected {
                        detail: format!(
                            "keyboard backlight write failed ({error}); rollback also failed ({rollback})"
                        ),
                    }),
                }
            }
        }
    }
}

/// Two keyboard-backlight channels in priority order.
///
/// The ACPI `KBLC` method is the machine's own interface; `LENOVO_LIGHTING_METHOD`
/// is the WMI lighting plane some models publish instead. Routing only falls
/// back when the primary channel is absent (`Unsupported` /
/// `ChannelUnavailable`) — a device-level error from the primary channel is
/// reported, not hidden, and the primary error survives even when the fallback
/// fails too.
#[derive(Debug)]
pub struct WindowsKeyboardSources<A, B> {
    primary: A,
    secondary: B,
}

impl<A, B> WindowsKeyboardSources<A, B> {
    #[must_use]
    pub const fn new(primary: A, secondary: B) -> Self {
        Self { primary, secondary }
    }
}

fn channel_absent(error: &LctrlError) -> bool {
    matches!(
        error,
        LctrlError::Unsupported { .. } | LctrlError::ChannelUnavailable { .. }
    )
}

impl<A, B> WindowsKeyboardSources<A, B>
where
    A: KeyboardControl,
    B: KeyboardControl,
{
    fn route<T>(
        &self,
        primary: impl Fn(&A) -> Result<T>,
        secondary: impl Fn(&B) -> Result<T>,
    ) -> Result<T> {
        match primary(&self.primary) {
            Err(error) if channel_absent(&error) => match secondary(&self.secondary) {
                Ok(value) => Ok(value),
                Err(_) => Err(error),
            },
            other => other,
        }
    }
}

impl<A, B> KeyboardControl for WindowsKeyboardSources<A, B>
where
    A: KeyboardControl,
    B: KeyboardControl,
{
    fn backlight_state(&self) -> Result<BacklightState> {
        self.route(A::backlight_state, B::backlight_state)
    }

    fn set_backlight(
        &self,
        level: u8,
        effect: LightingEffect,
        apply: ApplyMode,
    ) -> Result<ChangeReport<BacklightState>> {
        self.route(
            |service| service.set_backlight(level, effect, apply),
            |service| service.set_backlight(level, effect, apply),
        )
    }
}
