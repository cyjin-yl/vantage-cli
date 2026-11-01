//! Self-authored NDR descriptor generation.
//!
//! The RPC interfaces this crate speaks (the DAX control endpoint and the
//! Elevoc control endpoint) are described by MIDL-generated interpreter
//! tables: a proc format string, per-opnum offsets into it, and a type
//! format string. Copying those tables out of a vendor binary would embed
//! bytes we do not own, so both transports describe their used call surface
//! semantically (see [`dolby`] and [`elevoc`]) and this module turns that
//! description into the tables `NdrClientCall3` expects.
//!
//! The encodings follow the public DCE 1.1 NDR `-Oicf` grammar (the
//! `NDR_PROC_HEADER_*` / `NDR_PARAM_OIF` / `FC_*` elements, as implemented by
//! the public Wine and ReactOS headers). NDR encodings are canonical, so a
//! correctly generated descriptor coincides with any other implementation of
//! the same signature; what matters here is that the bytes are produced from
//! our own description of the interface rather than transcribed from a
//! proprietary image.
//!
//! The constants below are the shared grammar elements used by both
//! generators.
//!
//! The generators are only called by the Windows transports, so on other
//! platforms the crate sees them as unused even though their unit tests
//! still exercise them there.
#![allow(dead_code)]

pub mod dolby;
pub mod elevoc;

/// `FC_BIND_PRIMITIVE` — the procedure binds primitive (non-object) types.
pub(crate) const FC_BIND_PRIMITIVE: u8 = 0x32;
/// `FC_C_WSTRING` — the conformant wide-character string type character.
pub(crate) const FC_C_WSTRING: u8 = 0x25;
/// `FC_STRING_SIZED` — the string carries an explicit conformance count.
pub(crate) const FC_STRING_SIZED: u8 = 0x44;
/// `FC_CONSTANT_CONFORMANCE` — the conformance count is a compile-time
/// constant encoded in the descriptor.
pub(crate) const FC_CONSTANT_CONFORMANCE: u8 = 0x40;

/// `Oi_HAS_RPCFLAGS` — the procedure header carries the RPC flags word.
pub(crate) const OI_HAS_RPCFLAGS: u8 = 0x08;
/// `Oi_USE_NEW_INIT_ROUTINES` — the interpreter uses the newer init routines.
pub(crate) const OI_USE_NEW_INIT_ROUTINES: u8 = 0x40;
/// `Oi2_HAS_EXTENSIONS` — the procedure header has an extensions block.
pub(crate) const OI2_HAS_EXTENSIONS: u8 = 0x40;
/// `Oi2_SERVER_MUST_SIZE` — the server must size the parameter.
pub(crate) const OI2_SERVER_MUST_SIZE: u8 = 0x01;
/// `Oi2_CLIENT_MUST_SIZE` — the client must size the parameter.
pub(crate) const OI2_CLIENT_MUST_SIZE: u8 = 0x02;
/// `EXT2_HAS_NEW_CORR_DESC` — the extension block uses the new correlation
/// descriptor layout.
pub(crate) const EXT2_HAS_NEW_CORR_DESC: u8 = 0x01;
/// `EXT2_CLIENT_CORR_CHECK` — the client validates the correlation
/// descriptor.
pub(crate) const EXT2_CLIENT_CORR_CHECK: u8 = 0x02;
/// `EXT2_SERVER_CORR_CHECK` — the server validates the correlation
/// descriptor.
pub(crate) const EXT2_SERVER_CORR_CHECK: u8 = 0x04;

/// `PARAM_ATTRIBUTES` bit: the parameter must be sized by the caller.
pub(crate) const ATTR_MUST_SIZE: u16 = 0x0001;
/// `PARAM_ATTRIBUTES` bit: the receiver frees the parameter.
pub(crate) const ATTR_MUST_FREE: u16 = 0x0002;
/// `PARAM_ATTRIBUTES` bit: the parameter is passed in.
pub(crate) const ATTR_IS_IN: u16 = 0x0008;
/// `PARAM_ATTRIBUTES` bit: the parameter is returned.
pub(crate) const ATTR_IS_OUT: u16 = 0x0010;
