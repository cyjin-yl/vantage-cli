/// Build a NUL-terminated UTF-16 literal at compile time.
///
/// The Win32 wide APIs take `*const u16`; a narrow byte string cast to that
/// pointer type would be read as mojibake, so every class name, window title
/// and font face is widened here instead. The literals used are ASCII, so one
/// `u16` per input byte suffices.
macro_rules! wide {
    ($s:literal) => {{
        const LEN: usize = $s.len() + 1;
        const fn widen() -> [u16; LEN] {
            let bytes = $s.as_bytes();
            let mut out = [0u16; LEN];
            let mut i = 0;
            while i < bytes.len() {
                out[i] = bytes[i] as u16;
                i += 1;
            }
            out
        }
        widen()
    }};
}

pub(crate) use wide;

#[cfg(test)]
mod tests {
    const SAMPLE: &[u16] = &wide!("vantage");

    #[test]
    fn widens_ascii_and_terminates_with_nul() {
        assert_eq!(SAMPLE, &[0x76, 0x61, 0x6e, 0x74, 0x61, 0x67, 0x65, 0]);
    }
}
