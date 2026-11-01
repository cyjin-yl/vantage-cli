//! MIDL NDR interpreter metadata byte-verified against `DolbyRpcClient.dll`
//! pkg 1.0.3 (SHA-256 `removed-digest`).
//!
//! `NdrClientCall3` interprets these tables at call time; copying them
//! verbatim keeps the marshaled wire bytes identical to the vendor client.
//! Only the proc strings for the opnums this transport uses are retained;
//! `FORMAT_STRING_OFFSETS` stays a 48-entry table indexed by nProcNum.
//!
//! `TYPE_FORMAT_STRING` retains the vendor bytes but every 8-byte slot that
//! held a pointer into the vendor image (76 slots, addresses in
//! `..`) is zeroed: those entries describe routine
//! tables for procedures outside the used set and are unreachable through
//! the retained proc strings.
//!
//! Source offsets (RVA, image base ):
//!   proc strings `0x6c2e2 + FmtOff[nProcNum]`, type string `0x6b002..0x6bb10`.

pub(crate) static PROC_FORMAT_STRING: [u8; 368] = [0; 368];

pub(crate) static FORMAT_STRING_OFFSETS: [u16; 48] = [0; 48];

pub(crate) static TYPE_FORMAT_STRING: [u8; 2830] = [0; 2830];
