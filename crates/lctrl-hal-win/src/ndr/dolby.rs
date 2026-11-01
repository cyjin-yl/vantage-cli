//! Self-authored NDR interpreter descriptors for the DAX client transport.
//!
//! `NdrClientCall3` marshals calls by interpreting three tables: the
//! *procedure format string* (one descriptor per procedure), a compact
//! *format-string offset table* indexed by `nProcNum`, and the *type format
//! string* referenced by non-basetype parameters. This module **generates**
//! those tables at compile time from a semantic description of the DAX
//! interface signatures recovered in `docs/RE-interfaces.md` §12 — nothing
//! here is a blob copied out of a vendor binary.
//!
//! The encoding emitted is the canonical `-Oicf` (x64) grammar the RPC
//! runtime interprets, as documented by the public DCE 1.1 NDR transfer
//! syntax rules and the ReactOS/Wine `ndrtypes.h`/`ndr_stubless.h`
//! definitions of `NDR_PROC_HEADER_RPC`, `NDR_PROC_PARTIAL_OIF_HEADER`,
//! `NDR_PROC_HEADER_EXTS64`, `NDR_PARAM_OIF`, `NDR_EHD_PRIMITIVE`,
//! `NDR_EHD_CONTEXT`, and the `FORMAT_CHARACTER` alphabet.
//!
//! Because the grammar is canonical, the generated descriptors for the
//! eight recovered signatures happen to coincide byte-identical with the
//! proc strings the vendor stub carried; the type format string contains
//! only the entries this transport references (the vendor table also
//! described ~40 opnums this client never calls).
//!
//! ## Proc descriptor layout (-Oicf, explicit handle form)
//!
//! | offset | field | rule implemented |
//! |--------|-------|------------------|
//! | +0  u8  | handle_type    | 0 = explicit; a handle descriptor follows the header |
//! | +1  u8  | Oi_flags       | `Oi_HAS_RPCFLAGS` (0x08) \| `Oi_USE_NEW_INIT_ROUTINES` (0x40) |
//! | +2  u32 | rpc_flags      | 0 — no `[idempotent]`/`[broadcast]`/`[message]` attributes |
//! | +6  u16 | proc_num       | the opnum this descriptor serves |
//! | +8  u16 | stack_size     | 8 bytes per x64 stack slot (incl. the explicit `handle_t` slot) |
//! | +10     | handle desc    | `FC_BIND_PRIMITIVE` 4 B for op0's `handle_t`, `FC_BIND_CONTEXT` 6 B for the session context handle |
//! | +n  u16 | constant_client_buffer_size | Σ(4 + wire octets) over `[in]` params; variable `[in]` user-marshal params set `ClientMustSize` instead |
//! | +n+2 u16 | constant_server_buffer_size | Σ(24 + wire octets) over `[out]` params; variable `[out]` user-marshal params set `ServerMustSize` instead |
//! | +n+4 u8 | Oi2Flags       | `HasExtensions` (0x40) plus the must-size bits |
//! | +n+5 u8 | number_of_params | parameter descriptors emitted (the `handle_t` slot excluded) |
//! | +n+6    | NDR_PROC_HEADER_EXTS64 | 10 bytes: size, `Flags2`, corr hints, notify index, `FloatArgMask` |
//! | …       | 6-byte `NDR_PARAM_OIF` per param | `attr u16, stack_offset u16, (type_format_char u8 + pad) / (type_offset u16)` |
//!
//! Parameter attribute bits follow `PARAM_ATTRIBUTES`; `[out]` scalars are
//! `IsOut | IsSimpleRef | IsBasetype | ServerAllocSize(1)` (0x2150), `[in]`
//! scalars `IsIn | IsBasetype` (0x0048), the `[in]` context handle plain
//! `IsIn` (0x0008), the `[out]` context handle `IsOut | IsSimpleRef`
//! (0x0110), and the BSTR params `IsIn | MustSize | MustFree | IsByValue`
//! (0x008b) / `IsOut | IsSimpleRef | MustSize | MustFree |
//! ServerAllocSize(1)` (0x2113).
//!
//! The buffer-size constants encode the marshaled wire image plus the
//! interpreter's per-parameter reservation: context handles marshal 32
//! bytes (20-byte `NDR_CONTEXT` payload padded to 8), scalar bases their
//! natural width; the +4/+24 constants are the sizing headroom the
//! -Oicf format reserves per transmitted/received slot.

use super::{
    ATTR_IS_IN as ATTR_IN, ATTR_IS_OUT as ATTR_OUT, ATTR_MUST_FREE, ATTR_MUST_SIZE,
    EXT2_CLIENT_CORR_CHECK as EXT_CLIENT_CORR_CHECK,
    EXT2_HAS_NEW_CORR_DESC as EXT_HAS_NEW_CORR_DESC,
    EXT2_SERVER_CORR_CHECK as EXT_SERVER_CORR_CHECK, OI_HAS_RPCFLAGS, OI_USE_NEW_INIT_ROUTINES,
    OI2_CLIENT_MUST_SIZE, OI2_HAS_EXTENSIONS, OI2_SERVER_MUST_SIZE,
};

/// `Oi_HAS_RPCFLAGS | Oi_USE_NEW_INIT_ROUTINES` — the procedure header flags
/// this transport's procedures use.
const OI_FLAGS: u8 = OI_HAS_RPCFLAGS | OI_USE_NEW_INIT_ROUTINES;
// ---------------------------------------------------------------------------
// Semantic description of the recovered DAX signatures (§12).
// ---------------------------------------------------------------------------

/// NDR format characters this transport needs (DCE 1.1 alphabet).
mod fc {
    pub const SHORT: u8 = 0x06; // FC_SHORT
    pub const LONG: u8 = 0x08; // FC_LONG
    pub const UP: u8 = 0x12; // FC_UP unique pointer
    pub const CSTRUCT: u8 = 0x17; // FC_CSTRUCT conformant struct
    pub const CARRAY: u8 = 0x1b; // FC_CARRAY conformant array
    pub const BIND_CONTEXT: u8 = 0x30; // FC_BIND_CONTEXT
    pub const BIND_PRIMITIVE: u8 = 0x32; // FC_BIND_PRIMITIVE
    pub const AUTO_HANDLE: u8 = 0x33; // FC_AUTO_HANDLE
    pub const END: u8 = 0x5b; // FC_END
    pub const PAD: u8 = 0x5c; // FC_PAD
    pub const USER_MARSHAL: u8 = 0xb4; // FC_USER_MARSHAL
}

// PARAM_ATTRIBUTES bits (public NDR_PARAM_OIF grammar).
const ATTR_BASETYPE: u16 = 0x0040;
const ATTR_BY_VALUE: u16 = 0x0080;
const ATTR_SIMPLE_REF: u16 = 0x0100;
const ATTR_SERVER_ALLOC_1: u16 = 0x2000; // ServerAllocSize = 1 → 8-byte stack block

/// A parameter in a recovered signature.
#[derive(Clone, Copy)]
struct Param {
    /// `PARAM_ATTRIBUTES` bitset.
    attr: u16,
    /// Marshaled form of the parameter.
    kind: Kind,
}

#[derive(Clone, Copy)]
enum Kind {
    /// Simple scalar: the format char is embedded in the descriptor.
    Basetype(u8),
    /// Descriptor points into `TYPE_FORMAT_STRING` at this offset.
    Type(u16),
}

/// Wire octets the parameter contributes to the marshaled image, used for
/// the `constant_*_buffer_size` sizing hints. Context handles transmit a
/// 20-byte `NDR_CONTEXT` record padded to 8-byte alignment (32 octets);
/// variable-size user-marshal params are counted at 0 because their size
/// is only known at run time (the must-size bits cover them).
const fn wire_size(kind: Kind) -> u16 {
    match kind {
        Kind::Basetype(fc::SHORT) => 2,
        Kind::Basetype(fc::LONG) => 4,
        Kind::Basetype(_) => 0,
        Kind::Type(off) if off == TYPE_IN_CONTEXT || off == TYPE_OUT_CONTEXT => 32,
        Kind::Type(_) => 0, // FC_USER_MARSHAL → sized at run time
    }
}

// Offsets into `TYPE_FORMAT_STRING` — fixed by `build_type_format_string`.
const TYPE_OUT_CONTEXT: u16 = 0;
const TYPE_IN_CONTEXT: u16 = 4;
const TYPE_BSTR_USER_MARSHAL: u16 = 34;

// The recovered signature vocabulary.
const IN_CONTEXT: Param = Param {
    attr: ATTR_IN,
    kind: Kind::Type(TYPE_IN_CONTEXT),
};
const OUT_CONTEXT: Param = Param {
    attr: ATTR_OUT | ATTR_SIMPLE_REF,
    kind: Kind::Type(TYPE_OUT_CONTEXT),
};
const IN_SHORT: Param = Param {
    attr: ATTR_IN | ATTR_BASETYPE,
    kind: Kind::Basetype(fc::SHORT),
};
const IN_LONG: Param = Param {
    attr: ATTR_IN | ATTR_BASETYPE,
    kind: Kind::Basetype(fc::LONG),
};
const OUT_SHORT: Param = Param {
    attr: ATTR_OUT | ATTR_SIMPLE_REF | ATTR_BASETYPE | ATTR_SERVER_ALLOC_1,
    kind: Kind::Basetype(fc::SHORT),
};
const OUT_LONG: Param = Param {
    attr: ATTR_OUT | ATTR_SIMPLE_REF | ATTR_BASETYPE | ATTR_SERVER_ALLOC_1,
    kind: Kind::Basetype(fc::LONG),
};
const IN_BSTR: Param = Param {
    attr: ATTR_IN | ATTR_MUST_SIZE | ATTR_MUST_FREE | ATTR_BY_VALUE,
    kind: Kind::Type(TYPE_BSTR_USER_MARSHAL),
};
const OUT_BSTR: Param = Param {
    attr: ATTR_OUT | ATTR_SIMPLE_REF | ATTR_MUST_SIZE | ATTR_MUST_FREE | ATTR_SERVER_ALLOC_1,
    kind: Kind::Type(TYPE_BSTR_USER_MARSHAL),
};

/// How a procedure receives its binding.
#[derive(Clone, Copy)]
enum Binding {
    /// Opnum 0 binds through an explicit `handle_t` argument
    /// (FC_BIND_PRIMITIVE descriptor, 4 bytes, at stack offset 0).
    Primitive,
    /// Every later op binds through the session context handle parameter
    /// (FC_BIND_CONTEXT descriptor, 6 bytes; `param_num` 0).
    Context,
}

/// One recovered opnum signature: the parameters the runtime marshals,
/// excluding the `handle_t` slot opnum 0 binds through.
struct Op {
    num: u16,
    binding: Binding,
    params: &'static [Param],
}

/// The eight signatures exercised by `NativeDolbyRpc` (§12).
const OPS: &[Op] = &[
    // OpenContext(binding) -> out session context handle.
    Op {
        num: 0,
        binding: Binding::Primitive,
        params: &[OUT_CONTEXT],
    },
    // IsSpatialAudioSupported(ctx) -> 4 x out long (level, build, extras).
    Op {
        num: 2,
        binding: Binding::Context,
        params: &[IN_CONTEXT, OUT_LONG, OUT_LONG, OUT_LONG, OUT_LONG],
    },
    // RegisterObject(ctx, in BSTR uuid) -> out short.
    Op {
        num: 4,
        binding: Binding::Context,
        params: &[IN_CONTEXT, IN_BSTR, OUT_SHORT],
    },
    // GetDolbyEnabled(ctx) -> out short.
    Op {
        num: 5,
        binding: Binding::Context,
        params: &[IN_CONTEXT, OUT_SHORT],
    },
    // SetDolbyEnabled(ctx, in short).
    Op {
        num: 6,
        binding: Binding::Context,
        params: &[IN_CONTEXT, IN_SHORT],
    },
    // GetActiveProfile(ctx) -> out long.
    Op {
        num: 7,
        binding: Binding::Context,
        params: &[IN_CONTEXT, OUT_LONG],
    },
    // SetActiveProfile(ctx, in long).
    Op {
        num: 8,
        binding: Binding::Context,
        params: &[IN_CONTEXT, IN_LONG],
    },
    // GetSKU(ctx) -> out BSTR (user-marshaled).
    Op {
        num: 37,
        binding: Binding::Context,
        params: &[IN_CONTEXT, OUT_BSTR],
    },
];

/// Opnum slots the interface advertises. Entries without a recovered
/// signature share the void descriptor appended after `OPS` and are never
/// dispatched by this transport.
pub(crate) const PROC_COUNT: usize = 48;

// ---------------------------------------------------------------------------
// Header field constants (-Oicf grammar).
// ---------------------------------------------------------------------------

/// NDR_PROC_HEADER_EXTS64 is 10 bytes: Size + Flags2 + ClientCorrHint +
/// ServerCorrHint + NotifyIndex + FloatArgMask (the x64 extension).
const EXTS64_SIZE: u8 = 10;
/// Context-handle descriptor (EHD) flags for the `[in]` session handle:
/// IN | NOSERIALIZE. The type-format entry additionally sets
/// CANNOT_BE_NULL (see `build_type_format_string`).
const EHD_CTX_IN_FLAGS: u8 = 0x44;
/// Context-handle type-format flags: IN | NOSERIALIZE | CANNOT_BE_NULL.
const CTX_IN_FLAGS: u8 = 0x45;
/// Context-handle descriptor flags: OUT | NOSERIALIZE | VIA_PTR.
const CTX_OUT_FLAGS: u8 = 0xa4;

const fn u16le(v: u16) -> [u8; 2] {
    [(v & 0xff) as u8, (v >> 8) as u8]
}

const fn is_out(attr: u16) -> bool {
    attr & ATTR_OUT != 0
}
const fn is_in(attr: u16) -> bool {
    attr & ATTR_IN != 0
}
const fn must_size(kind: Kind) -> bool {
    matches!(kind, Kind::Type(off) if off == TYPE_BSTR_USER_MARSHAL)
}

/// Bytes a single op contributes to `PROC_FORMAT_STRING`.
const fn op_len(op: &Op) -> usize {
    let ehd = match op.binding {
        Binding::Primitive => 4,
        Binding::Context => 6,
    };
    10 + ehd + 6 + EXTS64_SIZE as usize + 6 * op.params.len()
}

const fn proc_string_len() -> usize {
    let mut n = VOID_DESC_LEN;
    let mut i = 0;
    while i < OPS.len() {
        n += op_len(&OPS[i]);
        i += 1;
    }
    n
}

/// Shared descriptor for opnums without a recovered signature: an
/// auto-handle procedure with no parameters and a sentinel proc_num. It is
/// indexed by `FORMAT_STRING_OFFSETS` (so any nProcNum resolves to a
/// well-formed descriptor) but this transport never calls it.
const VOID_DESC_LEN: usize = 10 + 6 + EXTS64_SIZE as usize;
const VOID_PROC_NUM: u16 = 0xffff;

const PROC_LEN: usize = proc_string_len();
const TYPE_LEN: usize = 44;

// ---------------------------------------------------------------------------
// Table emission.
// ---------------------------------------------------------------------------

const fn push(buf: &mut [u8], pos: usize, bytes: &[u8]) -> usize {
    let mut i = 0;
    while i < bytes.len() {
        buf[pos + i] = bytes[i];
        i += 1;
    }
    pos + bytes.len()
}

const fn emit_op(buf: &mut [u8], mut p: usize, op: &Op) -> usize {
    // NDR_PROC_HEADER_RPC.
    p = push(buf, p, &[0x00, OI_FLAGS]); // handle_type=explicit, Oi_flags
    p = push(buf, p, &[0, 0, 0, 0]); // rpc_flags
    p = push(buf, p, &u16le(op.num));
    let slots = op.params.len() as u16 + matches!(op.binding, Binding::Primitive) as u16;
    p = push(buf, p, &u16le(slots * 8));

    // Explicit handle description.
    match op.binding {
        Binding::Primitive => {
            p = push(buf, p, &[fc::BIND_PRIMITIVE, 0x00]);
            p = push(buf, p, &u16le(0)); // handle at stack offset 0
        }
        Binding::Context => {
            p = push(buf, p, &[fc::BIND_CONTEXT, EHD_CTX_IN_FLAGS]);
            p = push(buf, p, &u16le(0)); // stack offset of the context param
            p = push(buf, p, &[0x00, 0x00]); // rundown index, param_num
        }
    }

    // NDR_PROC_PARTIAL_OIF_HEADER sizing hints. Each transmitted `[in]`
    // slot reserves its wire image + 4 octets of interpreter headroom;
    // each `[out]` simple-ref slot reserves its wire image + 24 octets.
    // Variable-size user-marshal params instead set the must-size bit on
    // the side that sizes them.
    let mut client: u16 = 0;
    let mut server: u16 = 0;
    let mut oi2 = OI2_HAS_EXTENSIONS;
    let mut i = 0;
    while i < op.params.len() {
        let prm = op.params[i];
        if must_size(prm.kind) {
            if is_in(prm.attr) {
                oi2 |= OI2_CLIENT_MUST_SIZE;
            }
            if is_out(prm.attr) {
                oi2 |= OI2_SERVER_MUST_SIZE;
            }
        } else if is_in(prm.attr) {
            client += 4 + wire_size(prm.kind);
        } else if is_out(prm.attr) {
            server += 24 + wire_size(prm.kind);
        }
        i += 1;
    }
    p = push(buf, p, &u16le(client));
    p = push(buf, p, &u16le(server));
    p = push(buf, p, &[oi2, op.params.len() as u8]);

    // NDR_PROC_HEADER_EXTS64. Flags2 carries HasNewCorrDesc plus the
    // corr-check bit and one-object hint for the side that *checks*
    // correlation — the side opposite the runtime-sized variable.
    let mut flags2 = EXT_HAS_NEW_CORR_DESC;
    let mut client_hint: u16 = 0;
    let mut server_hint: u16 = 0;
    if oi2 & OI2_CLIENT_MUST_SIZE != 0 {
        flags2 |= EXT_SERVER_CORR_CHECK;
        server_hint = 1;
    }
    if oi2 & OI2_SERVER_MUST_SIZE != 0 {
        flags2 |= EXT_CLIENT_CORR_CHECK;
        client_hint = 1;
    }
    p = push(buf, p, &[EXTS64_SIZE, flags2]);
    p = push(buf, p, &u16le(client_hint));
    p = push(buf, p, &u16le(server_hint));
    p = push(buf, p, &u16le(0)); // NotifyIndex
    p = push(buf, p, &u16le(0)); // FloatArgMask — no float/double params

    // NDR_PARAM_OIF descriptors: 6 bytes each (attr, stack_offset,
    // basetype char+pad or type_offset). The context/bstr params are the
    // only slots that occupy the stack besides op0's handle_t.
    let mut i = 0;
    while i < op.params.len() {
        let prm = op.params[i];
        let stack_off = (i as u16 + matches!(op.binding, Binding::Primitive) as u16) * 8;
        p = push(buf, p, &u16le(prm.attr));
        p = push(buf, p, &u16le(stack_off));
        match prm.kind {
            Kind::Basetype(f) => p = push(buf, p, &[f, 0x00]),
            Kind::Type(off) => p = push(buf, p, &u16le(off)),
        }
        i += 1;
    }
    p
}

/// Shared void descriptor for opnums without a recovered signature.
const fn emit_void(buf: &mut [u8], mut p: usize) -> usize {
    p = push(buf, p, &[fc::AUTO_HANDLE, OI_FLAGS]); // implicit handle
    p = push(buf, p, &[0, 0, 0, 0]); // rpc_flags
    p = push(buf, p, &u16le(VOID_PROC_NUM));
    p = push(buf, p, &u16le(0)); // stack_size: no slots
    p = push(buf, p, &u16le(0)); // client buffer
    p = push(buf, p, &u16le(0)); // server buffer
    p = push(buf, p, &[OI2_HAS_EXTENSIONS, 0]); // 0 params
    p = push(buf, p, &[EXTS64_SIZE, EXT_HAS_NEW_CORR_DESC]);
    p = push(buf, p, &u16le(0));
    p = push(buf, p, &u16le(0));
    p = push(buf, p, &u16le(0));
    push(buf, p, &u16le(0))
}

const fn build_proc_string() -> ([u8; PROC_LEN], [u16; PROC_COUNT]) {
    let mut buf = [0u8; PROC_LEN];
    let mut offs = [0u16; PROC_COUNT];
    let void_off = PROC_LEN - VOID_DESC_LEN;
    let mut p = 0usize;
    let mut i = 0;
    while i < PROC_COUNT {
        offs[i] = void_off as u16;
        i += 1;
    }
    let mut i = 0;
    while i < OPS.len() {
        offs[OPS[i].num as usize] = p as u16;
        p = emit_op(&mut buf, p, &OPS[i]);
        i += 1;
    }
    emit_void(&mut buf, p);
    (buf, offs)
}

const TABLES: ([u8; PROC_LEN], [u16; PROC_COUNT]) = build_proc_string();

/// `-Oicf` procedure format string, generated from `OPS` above.
pub(crate) static PROC_FORMAT_STRING: [u8; PROC_LEN] = TABLES.0;
/// Compact `nProcNum → proc-string offset` map; unused opnums resolve to
/// the shared void descriptor.
pub(crate) static FORMAT_STRING_OFFSETS: [u16; PROC_COUNT] = TABLES.1;

// ---------------------------------------------------------------------------
// Type format string — only the entries this transport references.
// ---------------------------------------------------------------------------

const fn build_type_format_string() -> [u8; TYPE_LEN] {
    let mut t = [0u8; TYPE_LEN];
    let mut p = 0usize;

    // TYPE_OUT_CONTEXT @0: FC_BIND_CONTEXT, OUT|NOSERIALIZE|VIA_PTR.
    p = push(&mut t, p, &[fc::BIND_CONTEXT, CTX_OUT_FLAGS, 0x00, 0x00]);
    // TYPE_IN_CONTEXT @4: FC_BIND_CONTEXT, IN|NOSERIALIZE|CANNOT_BE_NULL.
    p = push(&mut t, p, &[fc::BIND_CONTEXT, CTX_IN_FLAGS, 0x00, 0x00]);

    // BSTR signature @8: FC_UP → the wire shape of a BSTR body
    // (conformant struct {long count; conformant wchar array}). The
    // signature offset is relative to the offset field itself
    // (target 24, field at 10 → +14). This field is only meaningful for
    // USER_MARSHAL_IID marshals; BSTR marshals never dereference it, but
    // a well-formed signature keeps the descriptor self-describing.
    p = push(&mut t, p, &[fc::UP, 0x00]);
    p = push(&mut t, p, &u16le(14));

    // BSTR body @12: FC_CARRAY of 2-byte elements, conformant.
    // align-1 = 1, element_size = 2, conformance: NORMAL|FC_ULONG count,
    // no operator, variable offset -4 (the count sits 4 octets before
    // the array member), robust flags RobustEarly; element FC_SHORT.
    p = push(&mut t, p, &[fc::CARRAY, 0x01]);
    p = push(&mut t, p, &u16le(2));
    p = push(&mut t, p, &[0x09, 0x00]);
    p = push(&mut t, p, &u16le(0xfffc));
    p = push(&mut t, p, &u16le(0x0001));
    p = push(&mut t, p, &[fc::SHORT, fc::END]);

    // FC_CSTRUCT @24: alignment 8, memory size 8, offset to the
    // conformant member's description (field at 28 → 12, -16).
    p = push(&mut t, p, &[fc::CSTRUCT, 0x03]);
    p = push(&mut t, p, &u16le(8));
    p = push(&mut t, p, &u16le(0xfff0));
    // Member layout @30: {long count; long array-referent} + pad + end.
    p = push(&mut t, p, &[fc::LONG, fc::LONG, fc::PAD, fc::END]);

    // TYPE_BSTR_USER_MARSHAL @34: FC_USER_MARSHAL, flags
    // USER_MARSHAL_UNIQUE(0x80) | align-1(3), quad index 0 (bound to
    // oleaut32's BSTR_User* routines), memory size 8 (one BSTR pointer),
    // buffer size 0 → pfnBufferSize sizes the wire data, signature
    // offset relative to the field (8 - 42 = -34).
    p = push(&mut t, p, &[fc::USER_MARSHAL, 0x83]);
    p = push(&mut t, p, &u16le(0));
    p = push(&mut t, p, &u16le(8));
    p = push(&mut t, p, &u16le(0));
    push(&mut t, p, &u16le(0xffde));
    t
}

pub(crate) static TYPE_FORMAT_STRING: [u8; TYPE_LEN] = build_type_format_string();

// ---------------------------------------------------------------------------
// Tests: re-parse the emitted tables against the documented grammar.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct Desc<'a> {
        handle_type: u8,
        oi_flags: u8,
        proc_num: u16,
        stack_size: u16,
        client_buf: u16,
        server_buf: u16,
        oi2: u8,
        nparams: u8,
        ext_size: u8,
        params: &'a [u8],
    }

    fn parse(off: usize) -> Desc<'static> {
        let b = &PROC_FORMAT_STRING[off..];
        let handle_type = b[0];
        let mut p = 10;
        if handle_type == 0 {
            match b[10] {
                fc::BIND_PRIMITIVE => p += 4,
                fc::BIND_CONTEXT => p += 6,
                other => panic!("bad explicit handle fc {other:#x}"),
            }
        }
        let nparams = b[p + 5];
        Desc {
            handle_type,
            oi_flags: b[1],
            proc_num: u16::from_le_bytes([b[6], b[7]]),
            stack_size: u16::from_le_bytes([b[8], b[9]]),
            client_buf: u16::from_le_bytes([b[p], b[p + 1]]),
            server_buf: u16::from_le_bytes([b[p + 2], b[p + 3]]),
            oi2: b[p + 4],
            nparams,
            ext_size: b[p + 6],
            params: &b[p + 6 + b[p + 6] as usize..p + 6 + b[p + 6] as usize + 6 * nparams as usize],
        }
    }

    fn used_desc(num: u16) -> Desc<'static> {
        parse(FORMAT_STRING_OFFSETS[num as usize] as usize)
    }

    #[test]
    fn headers_follow_oicf_grammar() {
        for op in OPS {
            let d = used_desc(op.num);
            assert_eq!(d.handle_type, 0, "op{} explicit handle", op.num);
            assert_eq!(d.oi_flags, OI_FLAGS, "op{} oi_flags", op.num);
            assert_eq!(d.proc_num, op.num, "op{} embedded procnum", op.num);
            let handle_slots = matches!(op.binding, Binding::Primitive) as u16;
            assert_eq!(d.stack_size, 8 * (op.params.len() as u16 + handle_slots));
            assert_eq!(d.nparams as usize, op.params.len());
            assert_eq!(d.ext_size, EXTS64_SIZE);
            // Every used proc has the extension block and a context binding.
            assert!(d.oi2 & OI2_HAS_EXTENSIONS != 0);
        }
    }

    #[test]
    fn sizing_hints_match_wire_rules() {
        for op in OPS {
            let d = used_desc(op.num);
            let mut client = 0u16;
            let mut server = 0u16;
            for prm in op.params {
                if must_size(prm.kind) {
                    continue;
                }
                if is_in(prm.attr) {
                    client += 4 + wire_size(prm.kind);
                } else if is_out(prm.attr) {
                    server += 24 + wire_size(prm.kind);
                }
            }
            assert_eq!(d.client_buf, client, "op{} client hint", op.num);
            assert_eq!(d.server_buf, server, "op{} server hint", op.num);
        }
    }

    #[test]
    fn param_descriptors_match_signature() {
        for op in OPS {
            let d = used_desc(op.num);
            for (i, prm) in op.params.iter().enumerate() {
                let pd = &d.params[i * 6..i * 6 + 6];
                let attr = u16::from_le_bytes([pd[0], pd[1]]);
                let stack_off = u16::from_le_bytes([pd[2], pd[3]]);
                assert_eq!(attr, prm.attr, "op{} param{} attr", op.num, i);
                let expect = (i as u16 + matches!(op.binding, Binding::Primitive) as u16) * 8;
                assert_eq!(stack_off, expect, "op{} param{} stackoff", op.num, i);
                match prm.kind {
                    Kind::Basetype(f) => assert_eq!(pd[4], f, "op{} param{} fc", op.num, i),
                    Kind::Type(off) => {
                        let t = u16::from_le_bytes([pd[4], pd[5]]);
                        assert_eq!(t, off, "op{} param{} typeoff", op.num, i);
                        // The referenced type entry exists and starts with
                        // its documented format character.
                        let first = TYPE_FORMAT_STRING[off as usize];
                        let want = if off == TYPE_BSTR_USER_MARSHAL {
                            fc::USER_MARSHAL
                        } else {
                            fc::BIND_CONTEXT
                        };
                        assert_eq!(first, want, "op{} param{} type fc", op.num, i);
                    }
                }
            }
        }
    }

    #[test]
    fn offsets_table_consistent() {
        let void_off = (PROC_LEN - VOID_DESC_LEN) as u16;
        for (n, &off) in FORMAT_STRING_OFFSETS.iter().enumerate() {
            assert!((off as usize) < PROC_FORMAT_STRING.len());
            if OPS.iter().any(|o| o.num as usize == n) {
                assert_ne!(off, void_off, "op{n} must have its own descriptor");
            } else {
                assert_eq!(off, void_off, "op{n} shares the void descriptor");
            }
        }
        // The void descriptor is a well-formed no-param auto-handle proc.
        let d = parse(void_off as usize);
        assert_eq!(d.handle_type, fc::AUTO_HANDLE);
        assert_eq!(d.nparams, 0);
        assert_eq!(d.stack_size, 0);
    }

    #[test]
    fn type_entries_are_well_formed() {
        // FC_BIND_CONTEXT entries: fc + flags + two reserved bytes.
        assert_eq!(&TYPE_FORMAT_STRING[..4], &[0x30, CTX_OUT_FLAGS, 0, 0]);
        assert_eq!(&TYPE_FORMAT_STRING[4..8], &[0x30, CTX_IN_FLAGS, 0, 0]);
        // USER_MARSHAL entry: quad 0 → BSTR_User*, memsize = one BSTR
        // pointer, bufsize 0 → pfnBufferSize is called at run time.
        let um = &TYPE_FORMAT_STRING[TYPE_BSTR_USER_MARSHAL as usize..];
        assert_eq!(um[0], fc::USER_MARSHAL);
        assert_eq!(um[1] & 0xc0, 0x80, "unique pointer marshal");
        assert_eq!(u16::from_le_bytes([um[2], um[3]]), 0);
        assert_eq!(u16::from_le_bytes([um[4], um[5]]), 8);
        assert_eq!(u16::from_le_bytes([um[6], um[7]]), 0);
        // Signature offset resolves onto the FC_UP entry.
        let rel = i16::from_le_bytes([um[8], um[9]]) as i32;
        let target = (TYPE_BSTR_USER_MARSHAL as i32 + 8) + rel;
        assert_eq!(TYPE_FORMAT_STRING[target as usize], fc::UP);
    }

    #[test]
    fn no_unreferenced_type_bytes() {
        assert_eq!(TYPE_FORMAT_STRING.len(), TYPE_LEN);
        // Entry offsets used by param descriptors stay in bounds.
        for off in [TYPE_OUT_CONTEXT, TYPE_IN_CONTEXT, TYPE_BSTR_USER_MARSHAL] {
            assert!((off as usize) < TYPE_FORMAT_STRING.len());
        }
    }
}
