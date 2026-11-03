//! Self-authored NDR32 metadata for the recovered HPD settings interface.
//!
//! The source of truth is the semantic signature below, not a vendor MIDL
//! byte array. Emission follows the public `-Oicf` x64 grammar used by the
//! adjacent Dolby generator. Every procedure returns a Win32 status in an
//! `FC_HYPER` return slot; that slot is metadata, not an extra call argument.
//! See `docs/evidence/hpd-rpc-re-20261102.md` for the two-sided contract.

use super::{
    ATTR_IS_IN, ATTR_IS_OUT, ATTR_MUST_FREE, ATTR_MUST_SIZE, EXT2_CLIENT_CORR_CHECK,
    EXT2_HAS_NEW_CORR_DESC, OI_HAS_RPCFLAGS, OI_USE_NEW_INIT_ROUTINES, OI2_HAS_EXTENSIONS,
    OI2_SERVER_MUST_SIZE,
};

mod fc {
    pub const WCHAR: u8 = 0x05;
    pub const LONG: u8 = 0x08;
    pub const ULONG: u8 = 0x09;
    pub const HYPER: u8 = 0x0b;
    pub const RP: u8 = 0x11;
    pub const UP: u8 = 0x12;
    pub const CARRAY: u8 = 0x1b;
    pub const BIND_CONTEXT: u8 = 0x30;
    pub const BIND_PRIMITIVE: u8 = 0x32;
    pub const AUTO_HANDLE: u8 = 0x33;
    pub const DEREFERENCE: u8 = 0x54;
    pub const END: u8 = 0x5b;
}

const ATTR_RETURN: u16 = 0x0020;
const ATTR_BASETYPE: u16 = 0x0040;
const ATTR_SIMPLE_REF: u16 = 0x0100;
const ATTR_SERVER_ALLOC_8: u16 = 0x2000;
const OI2_HAS_RETURN: u8 = 0x04;
const EXTS64_SIZE: usize = 10;

/// The aggregate getter's final scalar output is the element count for its
/// allocated UTF-16 array. Top-level correlation offsets are argument-stack
/// offsets, including the context at offset zero.
pub(crate) const SETTINGS_SCALARS: usize = 15;
const WCHAR_COUNT_STACK_OFFSET: u16 = SETTINGS_SCALARS as u16 * 8;

#[derive(Clone, Copy)]
enum Param {
    OutContext,
    InOutContext,
    InContext,
    InLong,
    OutLong,
    OutHyper,
    OutWchars,
    ReturnStatus,
}

impl Param {
    const fn attr(self) -> u16 {
        match self {
            Self::OutContext => ATTR_IS_OUT | ATTR_SIMPLE_REF,
            Self::InOutContext => ATTR_IS_IN | ATTR_IS_OUT | ATTR_SIMPLE_REF,
            Self::InContext => ATTR_IS_IN,
            Self::InLong => ATTR_IS_IN | ATTR_BASETYPE,
            Self::OutLong | Self::OutHyper => {
                ATTR_IS_OUT | ATTR_BASETYPE | ATTR_SIMPLE_REF | ATTR_SERVER_ALLOC_8
            }
            Self::OutWchars => ATTR_IS_OUT | ATTR_MUST_SIZE | ATTR_MUST_FREE | ATTR_SERVER_ALLOC_8,
            Self::ReturnStatus => ATTR_IS_OUT | ATTR_RETURN | ATTR_BASETYPE,
        }
    }

    const fn type_code(self) -> u16 {
        match self {
            Self::OutContext => TYPE_OUT_CONTEXT,
            Self::InOutContext => TYPE_INOUT_CONTEXT,
            Self::InContext => TYPE_IN_CONTEXT,
            Self::InLong | Self::OutLong => fc::LONG as u16,
            Self::OutHyper | Self::ReturnStatus => fc::HYPER as u16,
            Self::OutWchars => TYPE_WCHARS,
        }
    }

    // Conservative constant-buffer hints from each semantic parameter
    // class: a context reserves 32 octets, scalar widths are 4/8, and the
    // interpreter's in/ref/return headroom is accounted for separately.
    // These are sizing reserves, not bytes transcribed into the wire image.
    // Dynamic arrays take the server sizing pass.
    const fn client_hint(self) -> u16 {
        match self {
            Self::InContext => 4 + 32,
            Self::InOutContext => 24 + 32,
            Self::InLong => 4 + 4,
            _ => 0,
        }
    }

    const fn server_hint(self) -> u16 {
        match self {
            Self::OutContext | Self::InOutContext => 24 + 32,
            Self::OutLong => 24 + 4,
            Self::OutHyper => 24 + 8 + 4, // extra hyper alignment reserve
            Self::ReturnStatus => 8 + 8,
            _ => 0,
        }
    }
}

#[derive(Clone, Copy)]
enum Binding {
    Primitive,
    Context,
    ContextRef,
}

struct Op {
    num: u16,
    binding: Binding,
    params: &'static [Param],
}

const GET_SETTINGS_PARAMS: [Param; SETTINGS_SCALARS + 3] = {
    let mut p = [Param::OutLong; SETTINGS_SCALARS + 3];
    p[0] = Param::InContext;
    p[SETTINGS_SCALARS + 1] = Param::OutWchars;
    p[SETTINGS_SCALARS + 2] = Param::ReturnStatus;
    p
};

// Only operations used by the owned settings client are emitted. Callback
// registration and the server's unimplemented reserved operations are not
// exposed as pretend capabilities.
const OPS: &[Op] = &[
    Op {
        num: 0,
        binding: Binding::Primitive,
        params: &[Param::OutContext, Param::ReturnStatus],
    },
    Op {
        num: 1,
        binding: Binding::ContextRef,
        params: &[Param::InOutContext, Param::ReturnStatus],
    },
    Op {
        num: 4,
        binding: Binding::Context,
        params: &[Param::InContext, Param::OutLong, Param::ReturnStatus],
    },
    Op {
        num: 5,
        binding: Binding::Context,
        params: &[Param::InContext, Param::OutHyper, Param::ReturnStatus],
    },
    Op {
        num: 6,
        binding: Binding::Context,
        params: &GET_SETTINGS_PARAMS,
    },
    // ResetAllSetting: context only, no additional wire argument.
    Op {
        num: 7,
        binding: Binding::Context,
        params: &[Param::InContext, Param::ReturnStatus],
    },
    // Wake, approach distance, leave, leave wait, leave distance, video.
    Op {
        num: 8,
        binding: Binding::Context,
        params: &[Param::InContext, Param::InLong, Param::ReturnStatus],
    },
    Op {
        num: 9,
        binding: Binding::Context,
        params: &[Param::InContext, Param::InLong, Param::ReturnStatus],
    },
    Op {
        num: 10,
        binding: Binding::Context,
        params: &[Param::InContext, Param::InLong, Param::ReturnStatus],
    },
    Op {
        num: 11,
        binding: Binding::Context,
        params: &[Param::InContext, Param::InLong, Param::ReturnStatus],
    },
    Op {
        num: 12,
        binding: Binding::Context,
        params: &[Param::InContext, Param::InLong, Param::ReturnStatus],
    },
    Op {
        num: 13,
        binding: Binding::Context,
        params: &[Param::InContext, Param::InLong, Param::ReturnStatus],
    },
    Op {
        num: 14,
        binding: Binding::Context,
        params: &[Param::InContext, Param::InLong, Param::ReturnStatus],
    },
    Op {
        num: 15,
        binding: Binding::Context,
        params: &[Param::InContext, Param::InLong, Param::ReturnStatus],
    },
    Op {
        num: 16,
        binding: Binding::Context,
        params: &[Param::InContext, Param::InLong, Param::ReturnStatus],
    },
    Op {
        num: 17,
        binding: Binding::Context,
        params: &[Param::InContext, Param::InLong, Param::ReturnStatus],
    },
];

pub(crate) const PROC_COUNT: usize = 18;
const TYPE_OUT_CONTEXT: u16 = 0;
const TYPE_INOUT_CONTEXT: u16 = 4;
const TYPE_IN_CONTEXT: u16 = 8;
const TYPE_WCHARS: u16 = 12;
const TYPE_LEN: usize = TYPE_WCHARS as usize + 4 + 4 + 12;
const VOID_LEN: usize = 10 + 6 + EXTS64_SIZE;

const fn op_len(op: &Op) -> usize {
    let handle_len = if matches!(op.binding, Binding::Primitive) {
        4
    } else {
        6
    };
    10 + handle_len + 6 + EXTS64_SIZE + 6 * op.params.len()
}

const fn proc_len() -> usize {
    let mut n = VOID_LEN;
    let mut i = 0;
    while i < OPS.len() {
        n += op_len(&OPS[i]);
        i += 1;
    }
    n
}

const PROC_LEN: usize = proc_len();

const fn byte(buf: &mut [u8], p: &mut usize, v: u8) {
    buf[*p] = v;
    *p += 1;
}

const fn word(buf: &mut [u8], p: &mut usize, v: u16) {
    byte(buf, p, v as u8);
    byte(buf, p, (v >> 8) as u8);
}

const fn emit_op(buf: &mut [u8], mut p: usize, op: &Op) -> usize {
    byte(buf, &mut p, 0); // explicit handle description follows the RPC header
    byte(buf, &mut p, OI_HAS_RPCFLAGS | OI_USE_NEW_INIT_ROUTINES);
    word(buf, &mut p, 0); // rpc_flags
    word(buf, &mut p, 0);
    word(buf, &mut p, op.num);
    let binding_slot = matches!(op.binding, Binding::Primitive) as u16;
    word(buf, &mut p, (op.params.len() as u16 + binding_slot) * 8);
    match op.binding {
        Binding::Primitive => {
            byte(buf, &mut p, fc::BIND_PRIMITIVE);
            byte(buf, &mut p, 0);
            word(buf, &mut p, 0);
        }
        Binding::Context | Binding::ContextRef => {
            byte(buf, &mut p, fc::BIND_CONTEXT);
            // IN | NOSERIALIZE, plus OUT | VIA_PTR for the closing handle.
            byte(
                buf,
                &mut p,
                if matches!(op.binding, Binding::ContextRef) {
                    0xe4
                } else {
                    0x44
                },
            );
            word(buf, &mut p, 0);
            byte(buf, &mut p, 0); // rundown index
            byte(buf, &mut p, 0); // context parameter index
        }
    }
    let mut client = 0;
    let mut server = 0;
    let mut dynamic_out = false;
    let mut i = 0;
    while i < op.params.len() {
        client += op.params[i].client_hint();
        server += op.params[i].server_hint();
        dynamic_out |= matches!(op.params[i], Param::OutWchars);
        i += 1;
    }
    word(buf, &mut p, client);
    word(buf, &mut p, server);
    byte(
        buf,
        &mut p,
        OI2_HAS_EXTENSIONS | OI2_HAS_RETURN | if dynamic_out { OI2_SERVER_MUST_SIZE } else { 0 },
    );
    byte(buf, &mut p, op.params.len() as u8);
    byte(buf, &mut p, EXTS64_SIZE as u8);
    byte(
        buf,
        &mut p,
        EXT2_HAS_NEW_CORR_DESC
            | if dynamic_out {
                EXT2_CLIENT_CORR_CHECK
            } else {
                0
            },
    );
    word(buf, &mut p, dynamic_out as u16);
    word(buf, &mut p, 0); // server correlation hint
    word(buf, &mut p, 0); // notify index
    word(buf, &mut p, 0); // floating argument mask
    let mut i = 0;
    while i < op.params.len() {
        word(buf, &mut p, op.params[i].attr());
        word(buf, &mut p, (i as u16 + binding_slot) * 8);
        word(buf, &mut p, op.params[i].type_code());
        i += 1;
    }
    p
}

const fn build_proc() -> ([u8; PROC_LEN], [u16; PROC_COUNT]) {
    let mut buf = [0; PROC_LEN];
    let mut offsets = [(PROC_LEN - VOID_LEN) as u16; PROC_COUNT];
    let mut p = 0;
    let mut i = 0;
    while i < OPS.len() {
        offsets[OPS[i].num as usize] = p as u16;
        p = emit_op(&mut buf, p, &OPS[i]);
        i += 1;
    }
    // An uncalled slot is deliberately not a usable HPD operation.
    byte(&mut buf, &mut p, fc::AUTO_HANDLE);
    byte(&mut buf, &mut p, OI_HAS_RPCFLAGS | OI_USE_NEW_INIT_ROUTINES);
    word(&mut buf, &mut p, 0);
    word(&mut buf, &mut p, 0);
    word(&mut buf, &mut p, u16::MAX);
    word(&mut buf, &mut p, 0);
    word(&mut buf, &mut p, 0);
    word(&mut buf, &mut p, 0);
    byte(&mut buf, &mut p, OI2_HAS_EXTENSIONS);
    byte(&mut buf, &mut p, 0);
    byte(&mut buf, &mut p, EXTS64_SIZE as u8);
    byte(&mut buf, &mut p, EXT2_HAS_NEW_CORR_DESC);
    word(&mut buf, &mut p, 0);
    word(&mut buf, &mut p, 0);
    word(&mut buf, &mut p, 0);
    word(&mut buf, &mut p, 0);
    (buf, offsets)
}

const fn build_types() -> [u8; TYPE_LEN] {
    let mut buf = [0; TYPE_LEN];
    let mut p = 0;
    // Context flags: OUT|NOSERIALIZE|VIA_PTR; IN|OUT|NOSERIALIZE|
    // CANNOT_BE_NULL|VIA_PTR; IN|NOSERIALIZE|CANNOT_BE_NULL.
    let flags = [0xa4, 0xe5, 0x45];
    let mut i = 0;
    while i < flags.len() {
        byte(&mut buf, &mut p, fc::BIND_CONTEXT);
        byte(&mut buf, &mut p, flags[i]);
        word(&mut buf, &mut p, 0); // rundown/parameter indices
        i += 1;
    }
    // [out] wchar_t **: reference pointer to a nullable, allocated array.
    byte(&mut buf, &mut p, fc::RP);
    byte(&mut buf, &mut p, 0x14); // ALLOCED_ON_STACK | POINTER_DEREF
    word(&mut buf, &mut p, 2); // relative to this offset field -> FC_UP
    byte(&mut buf, &mut p, fc::UP);
    byte(&mut buf, &mut p, 0); // plain nullable unique pointer, no allocation override
    word(&mut buf, &mut p, 2); // -> FC_CARRAY
    byte(&mut buf, &mut p, fc::CARRAY);
    byte(&mut buf, &mut p, 1); // alignment minus one
    word(&mut buf, &mut p, 2); // sizeof(wchar_t)
    byte(&mut buf, &mut p, 0x20 | fc::ULONG); // TOP_LEVEL_CONFORMANCE | FC_ULONG
    byte(&mut buf, &mut p, fc::DEREFERENCE); // size_is(*count)
    word(&mut buf, &mut p, WCHAR_COUNT_STACK_OFFSET);
    word(&mut buf, &mut p, 1); // robust Early correlation
    byte(&mut buf, &mut p, fc::WCHAR);
    byte(&mut buf, &mut p, fc::END);
    buf
}

const TABLES: ([u8; PROC_LEN], [u16; PROC_COUNT]) = build_proc();
pub(crate) static PROC_FORMAT_STRING: [u8; PROC_LEN] = TABLES.0;
pub(crate) static FORMAT_STRING_OFFSETS: [u16; PROC_COUNT] = TABLES.1;
pub(crate) static TYPE_FORMAT_STRING: [u8; TYPE_LEN] = build_types();

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_at(b: &[u8], p: usize) -> u16 {
        u16::from_le_bytes([b[p], b[p + 1]])
    }

    fn record(op: usize) -> &'static [u8] {
        &PROC_FORMAT_STRING[FORMAT_STRING_OFFSETS[op] as usize..]
    }

    #[test]
    fn getter_outputs_and_status_have_distinct_widths() {
        for (op, output) in [(4, fc::LONG), (5, fc::HYPER)] {
            let b = record(op);
            assert_eq!(u16_at(b, 6), op as u16);
            assert_eq!(u16_at(b, 8), 24);
            assert_eq!(u16_at(b, 38), Param::OutLong.attr());
            assert_eq!(u16_at(b, 42), u16::from(output));
            assert_eq!(u16_at(b, 44), Param::ReturnStatus.attr());
            assert_eq!(u16_at(b, 48), u16::from(fc::HYPER));
        }
    }

    #[test]
    fn aggregate_array_count_references_the_fifteenth_scalar() {
        let b = record(6);
        assert_eq!(b[21], 18);
        assert_eq!(u16_at(b, 8), 144);
        assert_eq!(u16_at(b, 32 + 16 * 6), Param::OutWchars.attr());
        assert_eq!(u16_at(b, 32 + 16 * 6 + 2), 128);
        let t = &TYPE_FORMAT_STRING[TYPE_WCHARS as usize + 8..];
        assert_eq!(t[0], fc::CARRAY);
        assert_eq!(u16_at(t, 2), 2);
        assert_eq!(t[4], 0x20 | fc::ULONG);
        assert_eq!(t[5], fc::DEREFERENCE);
        assert_eq!(u16_at(t, 6), 120);
        assert_eq!(t[10], fc::WCHAR);
    }

    #[test]
    fn close_uses_an_inout_context_reference() {
        let b = record(1);
        assert_eq!(b[11], 0xe4);
        assert_eq!(u16_at(b, 32), Param::InOutContext.attr());
        assert_eq!(u16_at(b, 36), TYPE_INOUT_CONTEXT);
        assert_eq!(TYPE_FORMAT_STRING[TYPE_INOUT_CONTEXT as usize + 1], 0xe5);
    }
}
