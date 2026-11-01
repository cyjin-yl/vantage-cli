//! Self-authored NDR descriptor generator for `NdrClientCall3` (`-Oicf`
//! stubless clients).
//!
//! rpcrt4's marshaling engine interprets two byte tables per interface:
//!
//! * the **proc format string** — per-procedure headers followed by
//!   fixed-size parameter descriptors, reached per opnum through the
//!   `FmtStringOffset[]` table; and
//! * the **type format string** — shared type descriptors addressed by
//!   byte offset from the parameter descriptors.
//!
//! MIDL normally emits these tables from IDL. This module emits the same
//! canonical encoding from a *semantic description* of the recovered
//! signature, following the public format-string grammar that rpcrt4
//! implements (DCE 1.1 C706 chapter 14 for the wire syntax; the `-Oicf`
//! proc/type format-string contract documented in the Windows SDK "RPC NDR
//! Format Strings" reference — the same grammar reimplemented by the public
//! Wine/ReactOS interpreter). No field below is transcribed from a vendor
//! binary: each emission site names the grammar rule it implements. The
//! proc/type format strings are interpreter metadata; matching wire
//! semantics do not require every compiler to emit identical metadata.
//!
//! Layout of one proc-format-string record for a single-parameter
//! procedure (sizes in parentheses are the on-disk field widths; the
//! format is byte-packed, not C-struct aligned):
//!
//! ```text
//! 0x00  handle_type            (u8)   FC_BIND_PRIMITIVE = implicit caller handle
//! 0x01  Oi_flags               (u8)   Oi_HAS_RPCFLAGS | Oi_USE_NEW_INIT_ROUTINES
//! 0x02  rpc_flags              (u32)  RPCF_* call attributes
//! 0x06  proc_num               (u16)  procedure opnum
//! 0x08  stack_size             (u16)  total argument-stack bytes
//! 0x0a  constant_client_buf    (u16)  0 → interpreter must size
//! 0x0c  constant_server_buf    (u16)  0 → interpreter must size
//! 0x0e  Oi2 flags              (u8)   INTERPRETER_OPT_FLAGS
//! 0x0f  number_of_params       (u8)
//! 0x10  NDR_PROC_HEADER_EXTS64 (10B)  Win2000/x64 extension block
//! 0x1a  NDR_PARAM_OIF[n]       (6B each: attrs u16, stack_offset u16,
//!                                   type_offset/type_fc u16)
//! ```

use super::{
    ATTR_IS_IN, ATTR_IS_OUT, ATTR_MUST_FREE, ATTR_MUST_SIZE, EXT2_CLIENT_CORR_CHECK,
    EXT2_HAS_NEW_CORR_DESC, EXT2_SERVER_CORR_CHECK, FC_BIND_PRIMITIVE, FC_C_WSTRING,
    FC_CONSTANT_CONFORMANCE, FC_STRING_SIZED, OI_HAS_RPCFLAGS, OI_USE_NEW_INIT_ROUTINES,
    OI2_CLIENT_MUST_SIZE, OI2_HAS_EXTENSIONS, OI2_SERVER_MUST_SIZE,
};
// The grammar elements used below live in the parent module; the mapping is:
//
// * `FC_BIND_PRIMITIVE` — implicit primitive binding: the call uses the
//   `handle_t` the application created, reached through
//   `MIDL_STUB_DESC.IMPLICIT_HANDLE_INFO::pHandle`. The vendor client binds
//   `ncalrpc:[ElevocRpcEndpoint]` itself instead of letting MIDL auto-bind.
// * `FC_C_WSTRING` — conformant wide-character string (`[string] wchar_t`).
// * `FC_STRING_SIZED` — the string descriptor is followed by an explicit
//   conformance (size) descriptor instead of taking `MaxCount` from the
//   marshalled `ActualCount`.
// * `FC_CONSTANT_CONFORMANCE` (high nibble of the descriptor's flag byte) —
//   the conformance value is a compile-time constant packed into the
//   descriptor itself (`count = offset_lo16 | flag_byte1 << 16`), not read
//   from another parameter or stack slot.
// * `OI_HAS_RPCFLAGS` — the `rpc_flags` u32 is present in the header.
// * `OI_USE_NEW_INIT_ROUTINES` — the stub targets the post-Win2000
//   interpreter entry points (`NdrClientCall3` era).
// * `OI2_HAS_EXTENSIONS` / `OI2_CLIENT_MUST_SIZE` / `OI2_SERVER_MUST_SIZE` —
//   the client and server interpreters must run the sizing pass (no usable
//   `constant_client_buffer_size` shortcut) and a `NDR_PROC_HEADER_EXTS64`
//   extension block follows the OIF header.
// * `EXT2_HAS_NEW_CORR_DESC` / `EXT2_CLIENT_CORR_CHECK` /
//   `EXT2_SERVER_CORR_CHECK` — new-style (post-Win2000) correlation
//   descriptors are in use, which the `FC_CONSTANT_CONFORMANCE`-style
//   encoding below requires, and both sides arm the correlation check.
// * `ATTR_MUST_SIZE` / `ATTR_MUST_FREE` / `ATTR_IS_IN` / `ATTR_IS_OUT` —
//   the client interpreter must size this conformant payload, the server
//   interpreter must size (and later free) it, and the parameter marshals
//   on the request leg and unmarshals on the response leg.

/// Parameter direction in the `[in]`/`[out]` sense.
/// The full IDL domain is modelled; the Elevoc signature uses `InOut` and
/// the unit tests exercise `In`.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
    InOut,
}

/// Wire type of one parameter.
#[derive(Clone, Copy)]
pub enum WireType {
    /// `[in,out,string] wchar_t *` whose conformance is the compile-time
    /// constant `max_count` — canonical encoding of `size_is(max_count)`
    /// on a conformant wide string. On the wire the string travels as
    /// `{MaxCount u32, Offset u32, ActualCount u32, wchar_t data[]}`.
    ConformantWString {
        /// Declared element bound of the marshalled string. Must fit the
        /// signed 16-bit slot of the constant-conformance descriptor
        /// (the grammar packs the count into an `i16`; values ≥ 0x8000
        /// would sign-extend and are rejected at generation time).
        max_count: u16,
    },
}

impl WireType {
    /// Argument-stack footprint in bytes. Every type this generator emits
    /// is consumed as a pointer on x64 → one 8-byte slot.
    const fn stack_size(self) -> u16 {
        match self {
            Self::ConformantWString { .. } => 8,
        }
    }

    /// Emit this type's descriptor into the type format string and return
    /// nothing; non-basetype parameters are reached through `type_offset`
    /// rather than carrying an inline format character.
    fn emit_descriptor(&self, out: &mut Vec<u8>) {
        match *self {
            Self::ConformantWString { max_count } => {
                assert!(
                    max_count < 0x8000,
                    "constant-conformance count must fit the signed i16 slot"
                );
                out.push(FC_C_WSTRING);
                out.push(FC_STRING_SIZED);
                // Conformance descriptor: FC_CONSTANT_CONFORMANCE flag
                // nibble, operator/high word byte = 0, then the constant
                // element count as i16.
                out.push(FC_CONSTANT_CONFORMANCE);
                out.push(0x00);
                out.extend_from_slice(&(max_count as i16).to_le_bytes());
                // HasNewCorrDesc requires the /robust six-byte form.
                // A constant is known before unmarshalling: Early=1,
                // Split=IsIidIs=DontCheck=0. The remaining byte is reserved.
                // https://learn.microsoft.com/en-us/windows/win32/rpc/correlation-descriptors-tfs
                out.extend_from_slice(&1u16.to_le_bytes());
            }
        }
    }
}

/// One formal parameter in stack order.
pub struct Param {
    pub direction: Direction,
    /// Byte offset of this argument's slot on the interpreter stack.
    pub stack_offset: u16,
    pub ty: WireType,
}

/// One RPC procedure (one opnum).
pub struct Procedure {
    pub opnum: u16,
    pub params: Vec<Param>,
}

/// Semantic description of the interface's used call surface.
pub struct Signature {
    pub procedures: Vec<Procedure>,
}

/// Generated descriptor tables, owned by the caller so interior pointers
/// into them stay stable.
pub struct NdrDescriptors {
    /// Proc format string: `NDR_PROC_HEADER_RPC` + OIF tail + EXTS64 +
    /// `NDR_PARAM_OIF` records, one record per procedure, concatenated.
    pub proc_format: Vec<u8>,
    /// `FmtStringOffset` table: byte offset of each procedure's record
    /// inside `proc_format`, indexed by opnum position.
    pub proc_offsets: Vec<i16>,
    /// Type format string. Offsets 0-1 are reserved (offset 0 is the
    /// MIDL "null" type slot); the first real descriptor lands at 2.
    pub type_format: Vec<u8>,
}

/// Emit all three tables for `sig`.
pub fn generate(sig: &Signature) -> NdrDescriptors {
    // Pass 1: lay out the type format string so parameter descriptors can
    // store each type's byte offset.
    let mut type_format: Vec<u8> = vec![0x00, 0x00];
    let mut type_offsets: Vec<Vec<u16>> = Vec::with_capacity(sig.procedures.len());
    for proc in &sig.procedures {
        let mut offsets = Vec::with_capacity(proc.params.len());
        for param in &proc.params {
            let off = u16::try_from(type_format.len()).expect("type format string exceeds 64 KiB");
            param.ty.emit_descriptor(&mut type_format);
            offsets.push(off);
        }
        type_offsets.push(offsets);
    }
    // Descriptors contain u16 reads; keep the blob even-sized.
    if type_format.len() % 2 != 0 {
        type_format.push(0x00);
    }

    // Pass 2: emit each proc record, recording its offset.
    let mut proc_format: Vec<u8> = Vec::new();
    let mut proc_offsets: Vec<i16> = Vec::with_capacity(sig.procedures.len());
    for (pi, proc) in sig.procedures.iter().enumerate() {
        proc_offsets
            .push(i16::try_from(proc_format.len()).expect("proc format string exceeds 32 KiB"));
        emit_procedure(proc, &type_offsets[pi], &mut proc_format);
    }

    NdrDescriptors {
        proc_format,
        proc_offsets,
        type_format,
    }
}

/// `INTERPRETER_OPT_FLAGS`: both sides need a sizing pass whenever a
/// conformant parameter is marshalled in their direction; the Win2000
/// extension block is always emitted for this `NdrClientCall3` client.
fn oi2_flags(proc: &Procedure) -> u8 {
    let mut f = OI2_HAS_EXTENSIONS;
    if proc
        .params
        .iter()
        .any(|p| matches!(p.direction, Direction::In | Direction::InOut))
    {
        f |= OI2_CLIENT_MUST_SIZE;
    }
    if proc
        .params
        .iter()
        .any(|p| matches!(p.direction, Direction::Out | Direction::InOut))
    {
        f |= OI2_SERVER_MUST_SIZE;
    }
    f
}

/// `INTERPRETER_OPT_FLAGS2` for the extension block: new-style correlation
/// descriptors are always used by this generator; the check flags arm the
/// correlation package for the side(s) that marshal conformant data.
fn exts_flags2(proc: &Procedure) -> u8 {
    let mut f = EXT2_HAS_NEW_CORR_DESC;
    if proc
        .params
        .iter()
        .any(|p| matches!(p.direction, Direction::In | Direction::InOut))
    {
        f |= EXT2_CLIENT_CORR_CHECK;
    }
    if proc
        .params
        .iter()
        .any(|p| matches!(p.direction, Direction::Out | Direction::InOut))
    {
        f |= EXT2_SERVER_CORR_CHECK;
    }
    f
}

/// `PARAM_ATTRIBUTES` for one parameter: direction bits plus `MustSize`
/// whenever the type carries a conformance the interpreter must compute,
/// plus `MustFree` for anything the server side un-marshals/frees.
fn param_attributes(param: &Param) -> u16 {
    let mut a = match param.direction {
        Direction::In => ATTR_IS_IN | ATTR_MUST_SIZE,
        Direction::Out => ATTR_IS_OUT | ATTR_MUST_FREE,
        Direction::InOut => ATTR_IS_IN | ATTR_IS_OUT | ATTR_MUST_SIZE | ATTR_MUST_FREE,
    };
    if matches!(param.ty, WireType::ConformantWString { .. }) {
        a |= ATTR_MUST_SIZE;
    }
    a
}

/// Emit one proc-format-string record for `proc`.
fn emit_procedure(proc: &Procedure, type_offsets: &[u16], out: &mut Vec<u8>) {
    // NDR_PROC_HEADER_RPC (Oi_HAS_RPCFLAGS selects the variant that
    // carries rpc_flags; packed 10 bytes).
    out.push(FC_BIND_PRIMITIVE);
    out.push(OI_HAS_RPCFLAGS | OI_USE_NEW_INIT_ROUTINES);
    out.extend_from_slice(&0u32.to_le_bytes()); // rpc_flags: no RPCF_* attrs
    out.extend_from_slice(&proc.opnum.to_le_bytes());
    let stack_size: u16 = proc.params.iter().map(|p| p.ty.stack_size()).sum();
    out.extend_from_slice(&stack_size.to_le_bytes());

    // NDR_PROC_PARTIAL_OIF_HEADER (packed 6 bytes): zero constant buffer
    // sizes → the sizing pass always runs, consistent with MustSize.
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.push(oi2_flags(proc));
    out.push(u8::try_from(proc.params.len()).expect("more than 255 parameters is unsupported"));

    // NDR_PROC_HEADER_EXTS64 (packed 10 bytes). `Size` is the block's own
    // byte length: the interpreter advances the format cursor by it.
    out.push(10);
    out.push(exts_flags2(proc));
    // Correlation cache sizing hints (number of params each side checks);
    // consumed only as a cache hint by the correlation package.
    let client_hint = proc
        .params
        .iter()
        .filter(|p| matches!(p.direction, Direction::In | Direction::InOut))
        .count() as u16;
    let server_hint = proc
        .params
        .iter()
        .filter(|p| matches!(p.direction, Direction::Out | Direction::InOut))
        .count() as u16;
    out.extend_from_slice(&client_hint.to_le_bytes());
    out.extend_from_slice(&server_hint.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // NotifyIndex (no [notify])
    out.extend_from_slice(&0u16.to_le_bytes()); // FloatArgMask (IA64 only)

    // NDR_PARAM_OIF per parameter (6 bytes each).
    for (i, param) in proc.params.iter().enumerate() {
        out.extend_from_slice(&param_attributes(param).to_le_bytes());
        out.extend_from_slice(&param.stack_offset.to_le_bytes());
        out.extend_from_slice(&type_offsets[i].to_le_bytes());
    }
}

// ---------------------------------------------------------------------------
// The Elevoc endpoint signature (docs/RE-interfaces.md §14): one opnum-0
// dispatcher whose only argument is a JSON buffer exchanged as a
// `[in,out,string] wchar_t *`. The request JSON goes out as a UTF-16 string
// and the response lands in the same buffer.
// ---------------------------------------------------------------------------

/// Declared wire bound of the JSON buffer, in wide chars. The interface's
/// conformant string is sized by a compile-time constant (the service's
/// stub unmarshals up to this many elements into its reply buffer);
/// recovered value 0x5000. The local in/out storage must cover this bound,
/// including the terminating NUL, even when outgoing requests are shorter.
pub const ELEVOC_WIRE_BOUND_WCHARS: u16 = 0x5000;

/// Semantic description of the Elevoc opnum-0 stub:
/// `void dispatch([in,out,string,size_is(0x5000)] wchar_t *json)`.
pub fn elevoc_stub_signature() -> Signature {
    Signature {
        procedures: vec![Procedure {
            opnum: 0,
            params: vec![Param {
                direction: Direction::InOut,
                stack_offset: 0,
                ty: WireType::ConformantWString {
                    max_count: ELEVOC_WIRE_BOUND_WCHARS,
                },
            }],
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode the generated single-proc record field-by-field against the
    /// public `-Oicf` grammar and assert the signature's semantics came
    /// through (independent of any vendor bytes).
    #[test]
    fn elevoc_proc_record_decodes_to_the_signature() {
        let d = generate(&elevoc_stub_signature());
        assert_eq!(d.proc_offsets, vec![0]);
        let p = &d.proc_format[..];
        // 10B RPC header.
        assert_eq!(p[0], FC_BIND_PRIMITIVE, "implicit primitive handle_t");
        assert_eq!(
            p[1],
            OI_HAS_RPCFLAGS | OI_USE_NEW_INIT_ROUTINES,
            "rpc_flags present + modern interpreter"
        );
        assert_eq!(&p[2..6], &[0; 4], "no RPCF_* call attributes");
        assert_eq!(u16::from_le_bytes([p[6], p[7]]), 0, "opnum 0");
        assert_eq!(
            u16::from_le_bytes([p[8], p[9]]),
            8,
            "one pointer-sized stack slot"
        );
        // 6B OIF partial header.
        assert_eq!(&p[10..14], &[0; 4], "no constant buffer-size shortcuts");
        assert_eq!(
            p[14],
            OI2_CLIENT_MUST_SIZE | OI2_SERVER_MUST_SIZE | OI2_HAS_EXTENSIONS
        );
        assert_eq!(p[15], 1, "one parameter");
        // 10B EXTS64 block.
        assert_eq!(p[16], 10, "extension block length");
        assert_eq!(
            p[17],
            EXT2_HAS_NEW_CORR_DESC | EXT2_CLIENT_CORR_CHECK | EXT2_SERVER_CORR_CHECK
        );
        assert_eq!(&p[18..22], &[1, 0, 1, 0], "one corr hint each side");
        assert_eq!(&p[22..26], &[0; 4], "no notify, no float args");
        // 6B NDR_PARAM_OIF: [in,out] + sizing on both legs, stack slot 0,
        // type descriptor at offset 2.
        assert_eq!(
            u16::from_le_bytes([p[26], p[27]]),
            ATTR_MUST_SIZE | ATTR_MUST_FREE | ATTR_IS_IN | ATTR_IS_OUT
        );
        assert_eq!(u16::from_le_bytes([p[28], p[29]]), 0);
        assert_eq!(u16::from_le_bytes([p[30], p[31]]), 2);
        assert_eq!(d.proc_format.len(), 32, "one 0x20-byte record total");
    }

    #[test]
    fn elevoc_type_descriptor_decodes_to_bounded_wstring() {
        let d = generate(&elevoc_stub_signature());
        let t = &d.type_format[..];
        assert_eq!(&t[0..2], &[0, 0], "offsets 0-1 reserved");
        assert_eq!(t[2], FC_C_WSTRING, "conformant wide string");
        assert_eq!(t[3], FC_STRING_SIZED, "explicit size descriptor");
        assert_eq!(
            t[4] & 0xf0,
            FC_CONSTANT_CONFORMANCE,
            "compile-time constant conformance"
        );
        // Grammar rule: count = i16 @descriptor+2 | flag_byte1 << 16.
        let count = (i16::from_le_bytes([t[6], t[7]]) as i32) | ((t[5] as i32) << 16);
        assert_eq!(count, ELEVOC_WIRE_BOUND_WCHARS as i32);
        // HasNewCorrDesc advertises a six-byte correlation descriptor.
        // The interpreter reads robust_flags after the four-byte count,
        // even when every observed response is shorter than that bound.
        let robust = t
            .get(8..10)
            .expect("robust correlation flags must not lie past the type table");
        assert_eq!(u16::from_le_bytes([robust[0], robust[1]]), 1);
    }

    #[test]
    fn generator_derives_per_direction_flags() {
        // A pure [in] wstring: no server leg → no ServerMustSize /
        // ServerCorrCheck / MustFree.
        let sig = Signature {
            procedures: vec![Procedure {
                opnum: 3,
                params: vec![Param {
                    direction: Direction::In,
                    stack_offset: 0,
                    ty: WireType::ConformantWString { max_count: 4 },
                }],
            }],
        };
        let d = generate(&sig);
        let p = &d.proc_format[..];
        assert_eq!(u16::from_le_bytes([p[6], p[7]]), 3, "opnum preserved");
        assert_eq!(
            p[14],
            OI2_CLIENT_MUST_SIZE | OI2_HAS_EXTENSIONS,
            "only the client sizes"
        );
        assert_eq!(p[17], EXT2_HAS_NEW_CORR_DESC | EXT2_CLIENT_CORR_CHECK);
        assert_eq!(
            u16::from_le_bytes([p[26], p[27]]),
            ATTR_IS_IN | ATTR_MUST_SIZE
        );
    }

    #[test]
    fn generator_rejects_oversized_conformance() {
        let result = std::panic::catch_unwind(|| {
            generate(&Signature {
                procedures: vec![Procedure {
                    opnum: 0,
                    params: vec![Param {
                        direction: Direction::InOut,
                        stack_offset: 0,
                        ty: WireType::ConformantWString { max_count: 0x8000 },
                    }],
                }],
            });
        });
        assert!(result.is_err(), "0x8000 cannot fit the i16 count slot");
    }
}
