//! Extended Display Identification Data (EDID) decoding for the internal panel.
//!
//! Windows stores a monitor's EDID verbatim: the PnP monitor driver copies the
//! 128-byte base block plus every extension block it read into
//! `HKLM\SYSTEM\CurrentControlSet\Enum\DISPLAY\<hardware id>\<instance>\Device Parameters\EDID`
//! as a `REG_BINARY` value. That value is the panel's own data, so the identity
//! below is decoded here rather than taken from a vendor summary.
//!
//! Validation is deliberate. A value that is not a whole number of 128-byte
//! blocks, whose block checksums do not sum to zero, whose base block does not
//! start with the EDID magic, or whose declared block count disagrees with the
//! bytes actually read is refused instead of being partially interpreted: a
//! misparsed timing or serial is worse than an error, and stale `DISPLAY`
//! registry records are common (a disconnected monitor keeps its EDID, and one
//! instance can carry more than one class record).
//!
//! The EDID does not encode everything `panel info` reports. The OS output
//! technology (`D3DKMDT_VOT_INTERNAL`) is what says a panel is embedded, and it
//! is supplied by the caller; it only selects the connection classification.

use std::fmt;

use serde::Serialize;

use crate::{LctrlError, Result};

const BLOCK_LEN: usize = 128;
const DTD_LEN: usize = 18;
const DESCRIPTOR_COUNT: usize = 4;
const DESCRIPTOR_TEXT_OFFSET: usize = 5;
const DESCRIPTOR_TEXT_LEN: usize = 13;
const EDID_HEADER: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];
const EXTENSION_COUNT_OFFSET: usize = 126;
const DESCRIPTOR_TAG_SERIAL_NUMBER: u8 = 0xff;
const DESCRIPTOR_TAG_UNSPECIFIED_TEXT: u8 = 0xfe;
const DESCRIPTOR_TAG_RANGE_LIMITS: u8 = 0xfd;
const DESCRIPTOR_TAG_MONITOR_NAME: u8 = 0xfc;
/// EDID 1.4 digital interface codes (base block byte 20, bits 3:0). The code is
/// only defined for a digital input; an analog input leaves the nibble unused.
const INTERFACE_DVI: u8 = 1;
const INTERFACE_HDMI_A: u8 = 2;
const INTERFACE_HDMI_B: u8 = 3;
const INTERFACE_DISPLAY_PORT: u8 = 5;

/// How the panel is attached, as far as the EDID and the OS classification can
/// say. An EDID never carries an LVDS code: LVDS panels are digital with an
/// unspecified interface nibble, which is also what many eDP panels with an
/// EDID 1.3-style digital byte report, so the two cannot be told apart from the
/// block. `Unspecified` is therefore reported instead of guessing LVDS.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelConnection {
    /// DisplayPort interface code on a panel the OS classifies as internal.
    Edp,
    /// DisplayPort interface code on an external sink.
    DisplayPort,
    Hdmi,
    Dvi,
    /// The base block declares an analog input.
    Analog,
    /// Digital input whose interface nibble names no supported code (0 or a
    /// reserved value). On an embedded panel this is LVDS or eDP without a
    /// DisplayPort interface code; see [`PanelInput::interface_code`].
    Unspecified,
}

impl fmt::Display for PanelConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Edp => f.write_str("edp"),
            Self::DisplayPort => f.write_str("displayport"),
            Self::Hdmi => f.write_str("hdmi"),
            Self::Dvi => f.write_str("dvi"),
            Self::Analog => f.write_str("analog"),
            Self::Unspecified => f.write_str("unspecified"),
        }
    }
}

/// Base-block video input definition (byte 20), kept raw alongside the derived
/// [`PanelConnection`] so the classification stays auditable.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PanelInput {
    pub digital: bool,
    /// Bits per primary colour for a digital input; `None` for an analog input
    /// or an EDID that leaves the depth undefined.
    pub bit_depth: Option<u8>,
    /// EDID digital interface code (bits 3:0); 0 for an analog input.
    pub interface_code: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PanelResolution {
    pub width: u32,
    pub height: u32,
}

/// Millimetres. The base block stores centimetres, so the value is quantised to
/// 10 mm; it is the panel's active area, not the lid opening.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PanelPhysicalSize {
    pub width_mm: u16,
    pub height_mm: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PanelEdidVersion {
    pub major: u8,
    pub minor: u8,
}

/// EDID manufacture date. `week` is `None` when the block leaves it
/// unspecified (0) or uses the model-year encoding (0xFF); `model_year` then
/// says which of the two the base block meant.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PanelManufacture {
    pub year: u16,
    pub week: Option<u8>,
    pub model_year: bool,
}

/// Identity of one panel, decoded from its own EDID.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PanelIdentity {
    /// Three-letter PnP manufacturer code, e.g. `LEN`.
    pub manufacturer: String,
    /// Raw manufacturer id word (the code packs three five-bit letters).
    pub manufacturer_id: u16,
    /// Product code; together with `manufacturer` this is the monitor hardware
    /// id (`LEN8AD7`).
    pub product_code: u16,
    /// Monitor-name descriptor (`0xFC`), usually the panel part number.
    pub model_name: Option<String>,
    /// Unspecified-text descriptor (`0xFE`), usually the panel vendor.
    pub product_name: Option<String>,
    /// Base-block serial number; `None` when the block stores zero, which means
    /// "unused" rather than a serial of zero.
    pub serial_number: Option<u32>,
    /// Serial-number descriptor (`0xFF`).
    pub serial_text: Option<String>,
    pub manufacture: PanelManufacture,
    pub edid_version: PanelEdidVersion,
    /// Pixel geometry of the first detailed timing descriptor, which EDID
    /// defines as the preferred timing. `None` only when the base block carries
    /// no detailed timing at all.
    pub native_resolution: Option<PanelResolution>,
    /// Refresh rate of that preferred timing, rounded to whole hertz.
    pub native_refresh_hz: Option<u32>,
    pub physical_size: Option<PanelPhysicalSize>,
    /// Monitor range limits descriptor (`0xFD`), when the block carries one.
    pub limits: Option<PanelRangeLimits>,
    pub input: PanelInput,
    pub connection: PanelConnection,
    /// Whether the OS reported this display path as `D3DKMDT_VOT_INTERNAL`.
    pub internal: bool,
    /// Number of extension blocks that were read and checksum-verified.
    pub extension_blocks: u8,
}

/// Timings the panel advertises in its range-limits descriptor. These bound the
/// modes the panel accepts; they are not the current mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PanelRangeLimits {
    pub min_refresh_hz: u8,
    pub max_refresh_hz: u8,
    pub min_horizontal_khz: u8,
    pub max_horizontal_khz: u8,
    pub max_pixel_clock_mhz: u16,
}

/// Decode a panel's EDID value.
///
/// `internal` is the OS output-technology classification (`true` for
/// `D3DKMDT_VOT_INTERNAL`), not EDID data; it only distinguishes eDP from an
/// external DisplayPort sink.
pub fn decode_panel_edid(bytes: &[u8], internal: bool) -> Result<PanelIdentity> {
    if bytes.len() < BLOCK_LEN || bytes.len() % BLOCK_LEN != 0 {
        return Err(malformed(format!(
            "value is {} bytes, not a whole number of {BLOCK_LEN}-byte blocks",
            bytes.len()
        )));
    }
    for (index, block) in bytes.chunks_exact(BLOCK_LEN).enumerate() {
        let sum = block.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte));
        if sum != 0 {
            return Err(malformed(format!(
                "block {index} fails its checksum (sum 0x{sum:02x})"
            )));
        }
    }
    let base = &bytes[..BLOCK_LEN];
    if base[..EDID_HEADER.len()] != EDID_HEADER {
        return Err(malformed(format!(
            "header is {}, not the {} EDID magic",
            hex(&base[..EDID_HEADER.len()]),
            hex(&EDID_HEADER)
        )));
    }
    let declared_blocks = usize::from(base[EXTENSION_COUNT_OFFSET]) + 1;
    let blocks = bytes.len() / BLOCK_LEN;
    if declared_blocks != blocks {
        return Err(malformed(format!(
            "block declares {declared_blocks} block(s) but {blocks} were read"
        )));
    }
    let edid_version = PanelEdidVersion {
        major: base[18],
        minor: base[19],
    };
    if edid_version.major != 1 {
        return Err(malformed(format!(
            "version {}.{} does not use the EDID 1.x base layout",
            edid_version.major, edid_version.minor
        )));
    }

    let manufacturer_id = u16::from_be_bytes([base[8], base[9]]);
    if manufacturer_id & 0x8000 != 0 {
        return Err(malformed(format!(
            "manufacturer id 0x{manufacturer_id:04x} sets the reserved bit"
        )));
    }
    let mut letters = [0u8; 3];
    for (index, shift) in [10u16, 5, 0].into_iter().enumerate() {
        let letter = u8::try_from((manufacturer_id >> shift) & 0x1f).expect("five bits");
        if !(1..=26).contains(&letter) {
            return Err(malformed(format!(
                "manufacturer id 0x{manufacturer_id:04x} has a letter outside A-Z"
            )));
        }
        letters[index] = b'A' + letter - 1;
    }
    let manufacturer = String::from_utf8(letters.to_vec()).expect("ASCII letters");

    let product_code = u16::from_le_bytes([base[10], base[11]]);
    let serial_number = u32::from_le_bytes([base[12], base[13], base[14], base[15]]);
    let manufacture = PanelManufacture {
        year: 1990 + u16::from(base[17]),
        week: match base[16] {
            0 | 0xff => None,
            week => Some(week),
        },
        model_year: base[16] == 0xff,
    };

    let input_byte = base[20];
    let digital = input_byte & 0x80 != 0;
    let (bit_depth, interface_code) = if digital {
        (
            match (input_byte >> 4) & 0x07 {
                1 => Some(6),
                2 => Some(8),
                3 => Some(10),
                4 => Some(12),
                5 => Some(14),
                6 => Some(16),
                _ => None,
            },
            input_byte & 0x0f,
        )
    } else {
        (None, 0)
    };
    let connection = if !digital {
        PanelConnection::Analog
    } else {
        match interface_code {
            INTERFACE_DVI => PanelConnection::Dvi,
            INTERFACE_HDMI_A | INTERFACE_HDMI_B => PanelConnection::Hdmi,
            INTERFACE_DISPLAY_PORT if internal => PanelConnection::Edp,
            INTERFACE_DISPLAY_PORT => PanelConnection::DisplayPort,
            _ => PanelConnection::Unspecified,
        }
    };

    let width_cm = base[21];
    let height_cm = base[22];
    let physical_size = (width_cm != 0 && height_cm != 0).then(|| PanelPhysicalSize {
        width_mm: u16::from(width_cm) * 10,
        height_mm: u16::from(height_cm) * 10,
    });

    let mut model_name = None;
    let mut product_name = None;
    let mut serial_text = None;
    let mut limits = None;
    let mut native = None;
    for index in 0..DESCRIPTOR_COUNT {
        let start = 54 + index * DTD_LEN;
        let descriptor = &base[start..start + DTD_LEN];
        let pixel_clock_10khz = u16::from_le_bytes([descriptor[0], descriptor[1]]);
        if pixel_clock_10khz == 0 {
            let tag = descriptor[3];
            let text = descriptor_text(
                &descriptor[DESCRIPTOR_TEXT_OFFSET..DESCRIPTOR_TEXT_OFFSET + DESCRIPTOR_TEXT_LEN],
            );
            match tag {
                DESCRIPTOR_TAG_SERIAL_NUMBER if serial_text.is_none() => serial_text = text,
                DESCRIPTOR_TAG_UNSPECIFIED_TEXT if product_name.is_none() => product_name = text,
                DESCRIPTOR_TAG_MONITOR_NAME if model_name.is_none() => model_name = text,
                DESCRIPTOR_TAG_RANGE_LIMITS if limits.is_none() => {
                    limits = Some(PanelRangeLimits {
                        min_refresh_hz: descriptor[5],
                        max_refresh_hz: descriptor[6],
                        min_horizontal_khz: descriptor[7],
                        max_horizontal_khz: descriptor[8],
                        // The descriptor stores the maximum pixel clock in
                        // 10 MHz units.
                        max_pixel_clock_mhz: u16::from(descriptor[9]) * 10,
                    });
                }
                _ => {}
            }
            continue;
        }
        if native.is_some() {
            continue;
        }
        native = Some(decode_timing(descriptor, pixel_clock_10khz)?);
    }
    let (native_resolution, native_refresh_hz) = match native {
        Some((resolution, refresh_hz)) => (Some(resolution), Some(refresh_hz)),
        None => (None, None),
    };

    Ok(PanelIdentity {
        manufacturer,
        manufacturer_id,
        product_code,
        model_name,
        product_name,
        serial_number: (serial_number != 0).then_some(serial_number),
        serial_text,
        manufacture,
        edid_version,
        native_resolution,
        native_refresh_hz,
        physical_size,
        limits,
        input: PanelInput {
            digital,
            bit_depth,
            interface_code,
        },
        connection,
        internal,
        extension_blocks: u8::try_from(blocks - 1).unwrap_or(u8::MAX),
    })
}

/// Decode one 18-byte detailed timing descriptor into its pixel geometry and
/// whole-hertz refresh rate.
fn decode_timing(descriptor: &[u8], pixel_clock_10khz: u16) -> Result<(PanelResolution, u32)> {
    let horizontal_active =
        u32::from(descriptor[2]) | (u32::from(descriptor[4] & 0xf0) << 4);
    let horizontal_blank = u32::from(descriptor[3]) | (u32::from(descriptor[4] & 0x0f) << 8);
    let vertical_active = u32::from(descriptor[5]) | (u32::from(descriptor[7] & 0xf0) << 4);
    let vertical_blank = u32::from(descriptor[6]) | (u32::from(descriptor[7] & 0x0f) << 8);
    if horizontal_active == 0
        || horizontal_blank == 0
        || vertical_active == 0
        || vertical_blank == 0
    {
        return Err(malformed(format!(
            "preferred timing {} is not a complete timing ({horizontal_active}+{horizontal_blank} x {vertical_active}+{vertical_blank})",
            hex(descriptor)
        )));
    }
    let total_pixels = u64::from(horizontal_active + horizontal_blank)
        * u64::from(vertical_active + vertical_blank);
    let refresh_hz = (u64::from(pixel_clock_10khz) * 10_000 * 2 + total_pixels)
        / (2 * total_pixels);
    Ok((
        PanelResolution {
            width: horizontal_active,
            height: vertical_active,
        },
        u32::try_from(refresh_hz).unwrap_or(u32::MAX),
    ))
}

/// Decode a text descriptor: the 13 bytes are NUL/line-feed padded, and a byte
/// outside printable ASCII makes the text unrepresentable (`None`) rather than
/// silently transliterated.
fn descriptor_text(bytes: &[u8]) -> Option<String> {
    let text: Vec<u8> = bytes
        .iter()
        .copied()
        .take_while(|byte| *byte != b'\n' && *byte != 0)
        .collect();
    if text.iter().any(|byte| !(0x20..=0x7e).contains(byte)) {
        return None;
    }
    let text = String::from_utf8(text).ok()?;
    let text = text.trim_end_matches(' ');
    (!text.is_empty()).then(|| text.to_owned())
}

fn malformed(detail: impl Into<String>) -> LctrlError {
    LctrlError::ChannelUnavailable {
        channel: format!("EDID {}", detail.into()),
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// CTA-861 extension block tag in the EDID extension sequence.
const CTA_EXTENSION: u8 = 0x02;
/// Offset of the data-block collection inside a CTA extension block.
const CTA_DATA_BLOCKS: usize = 4;
/// Data block tag whose payload starts with an extended tag byte.
const CTA_TAG_EXTENDED: u8 = 7;
/// Extended tag: Colorimetry data block (CTA-861-F §7.5.5).
const CTA_EXTENDED_COLORIMETRY: u8 = 0x05;
/// Extended tag: HDR Static Metadata data block (CTA-861.3 §3.1).
const CTA_EXTENDED_HDR_STATIC_METADATA: u8 = 0x06;
/// The Colorimetry block's first payload byte marks BT.2020 RGB support in
/// bit 7; HDR presentation is defined on that colour volume.
const CTA_COLORIMETRY_BT2020_RGB: u8 = 0x80;
/// CTA-861-F (revision 3) is the first revision that can carry an HDR block.
const CTA_MIN_HDR_REVISION: u8 = 3;

/// `2^(r/32)` for `r` in `0..32`, in Q20 fixed point. The CTA-861 luminance
/// code is `50 * 2^(code/32)` nits, so one code step is a 32nd of an octave;
/// keeping the fractional part in integers makes the decoded nits reproducible
/// instead of libm-dependent.
const POW2_Q20: [u32; 32] = [
    1_048_576, 1_071_537, 1_095_000, 1_118_978, 1_143_480, 1_168_519, 1_194_106, 1_220_254,
    1_246_974, 1_274_279, 1_302_182, 1_330_696, 1_359_835, 1_389_611, 1_420_040, 1_451_135,
    1_482_910, 1_515_382, 1_548_564, 1_582_474, 1_617_125, 1_652_536, 1_688_722, 1_725_700,
    1_763_488, 1_802_103, 1_841_564, 1_881_889, 1_923_097, 1_965_207, 2_008_240, 2_052_214,
];

/// HDR capability a monitor declares in its own CTA-861 extension blocks.
///
/// The EOTF field is a bit set, not an ordinal: bit 0 is traditional gamma over
/// the SDR range, bit 1 traditional gamma over the HDR range, bit 2 SMPTE
/// ST 2084 (the HDR10 / PQ transfer function), bit 3 BT.2100 HLG. A monitor
/// that declares bit 2 accepts an HDR10 signal; one that declares only bit 1 or
/// bit 3 does not.
///
/// Luminance codes use the CTA-861 curve: maximum and average
/// `50 * 2^(code/32)` nits, minimum `max * (code/255)^2 / 100` nits. They are
/// reported in millinits alongside the raw codes so the declaration stays
/// auditable and the conversion stays exact.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PanelHdrCapability {
    pub eotf_sdr: bool,
    pub eotf_hdr10_pq: bool,
    pub eotf_hlg: bool,
    /// Bit 7 of the Colorimetry data block: BT.2020 RGB is accepted.
    pub bt2020_rgb: bool,
    /// The sink accepts static-metadata type 1 (CTA-861.3).
    pub static_metadata_type1: bool,
    pub max_luminance_code: Option<u8>,
    pub average_luminance_code: Option<u8>,
    pub min_luminance_code: Option<u8>,
    pub max_luminance_millinits: Option<u32>,
    pub average_luminance_millinits: Option<u32>,
    pub min_luminance_millinits: Option<u32>,
}

/// Decode the HDR capability a monitor declares, or `None` when its EDID
/// carries no CTA-861 HDR Static Metadata data block (an SDR-only sink).
///
/// The bytes are admitted exactly like [`decode_panel_edid`] admits them —
/// whole 128-byte blocks, per-block checksums, EDID magic, declared extension
/// count — so a stale `DISPLAY` record cannot be read as a capability. The
/// identity returned by that validation is discarded: `internal` only selects
/// the connection classification, which this function does not report.
///
/// A block whose declared length runs past the data-block collection, or an HDR
/// block that declares a partial luminance triple (4 or 5 payload bytes), is
/// refused rather than partially interpreted.
pub fn decode_panel_hdr_capability(bytes: &[u8]) -> Result<Option<PanelHdrCapability>> {
    decode_panel_edid(bytes, true)?;

    let mut hdr: Option<PanelHdrCapability> = None;
    let mut bt2020_rgb = false;
    for (index, block) in bytes.chunks_exact(BLOCK_LEN).enumerate().skip(1) {
        if block[0] != CTA_EXTENSION || block[1] < CTA_MIN_HDR_REVISION {
            continue;
        }
        let dtd_offset = usize::from(block[2]);
        if !(CTA_DATA_BLOCKS..=BLOCK_LEN).contains(&dtd_offset) {
            return Err(malformed(format!(
                "extension block {index} puts the timing descriptor area at {dtd_offset}, outside the block"
            )));
        }
        let mut offset = CTA_DATA_BLOCKS;
        while offset < dtd_offset {
            let header = block[offset];
            let length = usize::from(header & 0x1f);
            let tag = header >> 5;
            let payload = offset + 1;
            // A zero-length block would not advance the cursor; a block that
            // overruns the declared collection would read timing bytes as
            // capability bits.
            if length == 0 || payload + length > dtd_offset {
                return Err(malformed(format!(
                    "extension block {index} data block at offset {offset} declares {length} payload byte(s) past the data block collection"
                )));
            }
            if tag == CTA_TAG_EXTENDED && length >= 1 {
                match block[payload] {
                    CTA_EXTENDED_COLORIMETRY if length >= 2 => {
                        bt2020_rgb |= block[payload + 1] & CTA_COLORIMETRY_BT2020_RGB != 0;
                    }
                    CTA_EXTENDED_HDR_STATIC_METADATA if hdr.is_none() => {
                        // extended tag + EOTF + static-metadata descriptor
                        if length < 3 {
                            return Err(malformed(format!(
                                "extension block {index} HDR static metadata block declares {length} payload byte(s)"
                            )));
                        }
                        if length != 3 && length != 6 {
                            return Err(malformed(format!(
                                "extension block {index} HDR static metadata block declares {length} payload bytes; the luminance triple is all or nothing"
                            )));
                        }
                        let eotf = block[payload + 1];
                        let (max_code, average_code, min_code) = if length == 6 {
                            (
                                Some(block[payload + 3]),
                                Some(block[payload + 4]),
                                Some(block[payload + 5]),
                            )
                        } else {
                            (None, None, None)
                        };
                        let max_millinits = max_code.map(luminance_millinits);
                        hdr = Some(PanelHdrCapability {
                            eotf_sdr: eotf & 0x01 != 0,
                            eotf_hdr10_pq: eotf & 0x04 != 0,
                            eotf_hlg: eotf & 0x08 != 0,
                            bt2020_rgb: false,
                            static_metadata_type1: block[payload + 2] & 0x01 != 0,
                            max_luminance_code: max_code,
                            average_luminance_code: average_code,
                            min_luminance_code: min_code,
                            max_luminance_millinits: max_millinits,
                            average_luminance_millinits: average_code.map(luminance_millinits),
                            min_luminance_millinits: match (max_millinits, min_code) {
                                (Some(max), Some(code)) => Some(min_luminance_millinits(max, code)),
                                _ => None,
                            },
                        });
                    }
                    _ => {}
                }
            }
            offset = payload + length;
        }
    }
    Ok(hdr.map(|capability| PanelHdrCapability {
        bt2020_rgb,
        ..capability
    }))
}

/// Maximum/average luminance code to millinits: `50 * 2^(code/32)` nits.
fn luminance_millinits(code: u8) -> u32 {
    let octave = u32::from(code / 32);
    let fraction = POW2_Q20[usize::from(code % 32)];
    let millinits = 50_000u64 * (1u64 << octave) * u64::from(fraction);
    u32::try_from(millinits >> 20).unwrap_or(u32::MAX)
}

/// Minimum luminance code to millinits: `max * (code/255)^2 / 100` nits.
fn min_luminance_millinits(max_millinits: u32, code: u8) -> u32 {
    let numerator = u64::from(max_millinits) * u64::from(code) * u64::from(code);
    let denominator = 255u64 * 255 * 100;
    u32::try_from(numerator / denominator).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `Device Parameters\EDID` value of `DISPLAY\LEN8AD7\...` on the
    /// 21VG reference target: base block plus a CTA-861 and a DisplayID
    /// extension block.
    const REFERENCE_EDID: &str = concat!(
        "00ffffffffffff0030aed78a0000000000230104b51f1478030f95ae5243b0260f505400",
        "000001010101010101010101010101010101189900a0c08065703020660038c310000018",
        "000000fd001e78f0f04e010a202020202020000000fe004c454e4f564f0a202020202020",
        "000000fc004c454e31342e35334b0a20202002c902030f00e3058080e60605016a6a4800",
        "000000000000000000000000000000000000000000000000000000000000000000000000",
        "000000000000000000000000000000000000000000000000000000000000000000000000",
        "000000000000000000000000000000000000000000000000000000000000000000000000",
        "000000f6702079020020000c408bf66d140000000000190021001d310c9f07000c800780",
        "e1fa513da4b066620f024554d05fd05f00451378220014d6f50b05ff0b9f002f001f007f",
        "076400050005002600090600000000005000002e00060045d05fd05f810015741a000003",
        "531e7800206bff6bff7800000000ad00000000000000cc90",
    );

    /// The `Device Parameters\EDID` value of `DISPLAY\DELD0E6\...`: a stale
    /// record for a monitor that is no longer attached; its base block has the
    /// right length and header but does not sum to zero.
    const STALE_EDID: &str = concat!(
        "00ffffffffffff0010ace6d0555a4a30241d0104a53c2278fb6ce5a55550a0230b505400",
        "0200d1c0010101010101010101010101010158e300a0a0a029503020350055502100001a",
        "000000ff00374a51584259320a2020202020000000fc0053323731394447460a20202020",
        "000000fd00289bfafa40010a202020202020002c",
    );

    fn bytes(hex: &str) -> Vec<u8> {
        assert_eq!(hex.len() % 2, 0, "hex length");
        (0..hex.len())
            .step_by(2)
            .map(|index| {
                u8::from_str_radix(&hex[index..index + 2], 16).expect("hex digit pair")
            })
            .collect()
    }

    /// Recompute one block's checksum byte so a test can isolate a field-level
    /// defect from the block-level check.
    fn reseal(value: &mut [u8], block: usize) {
        let start = block * BLOCK_LEN;
        let sum = value[start..start + BLOCK_LEN - 1]
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
        value[start + BLOCK_LEN - 1] = 0u8.wrapping_sub(sum);
    }

    #[test]
    fn decodes_the_reference_panel() {
        let identity = decode_panel_edid(&bytes(REFERENCE_EDID), true).expect("reference EDID");
        assert_eq!(identity.manufacturer, "LEN");
        assert_eq!(identity.manufacturer_id, 0x30ae);
        assert_eq!(identity.product_code, 0x8ad7);
        assert_eq!(identity.model_name.as_deref(), Some("LEN14.53K"));
        assert_eq!(identity.product_name.as_deref(), Some("LENOVO"));
        assert_eq!(identity.serial_number, None);
        assert_eq!(identity.serial_text, None);
        assert_eq!(
            identity.manufacture,
            PanelManufacture {
                year: 2025,
                week: None,
                model_year: false
            }
        );
        assert_eq!(
            identity.edid_version,
            PanelEdidVersion {
                major: 1,
                minor: 4
            }
        );
        assert_eq!(
            identity.native_resolution,
            Some(PanelResolution {
                width: 3072,
                height: 1920
            })
        );
        assert_eq!(identity.native_refresh_hz, Some(60));
        assert_eq!(
            identity.physical_size,
            Some(PanelPhysicalSize {
                width_mm: 310,
                height_mm: 200
            })
        );
        assert_eq!(
            identity.limits,
            Some(PanelRangeLimits {
                min_refresh_hz: 30,
                max_refresh_hz: 120,
                min_horizontal_khz: 240,
                max_horizontal_khz: 240,
                max_pixel_clock_mhz: 780
            })
        );
        assert_eq!(
            identity.input,
            PanelInput {
                digital: true,
                bit_depth: Some(10),
                interface_code: 5
            }
        );
        assert_eq!(identity.connection, PanelConnection::Edp);
        assert!(identity.internal);
        assert_eq!(identity.extension_blocks, 2);
    }

    #[test]
    fn a_displayport_panel_that_is_not_internal_is_external_displayport() {
        let identity = decode_panel_edid(&bytes(REFERENCE_EDID), false).expect("reference EDID");
        assert_eq!(identity.connection, PanelConnection::DisplayPort);
        assert!(!identity.internal);
    }

    #[test]
    fn decodes_an_external_displayport_panel() {
        // A stale-but-intact record for a monitor that is not attached: the
        // block itself is valid, so it decodes, and the caller's
        // `internal = false` keeps it an external sink.
        let identity = decode_panel_edid(&bytes(STALE_EDID), false).expect("stale EDID");
        assert_eq!(identity.manufacturer, "DEL");
        assert_eq!(identity.product_code, 0xd0e6);
        assert_eq!(identity.model_name.as_deref(), Some("S2719DGF"));
        assert_eq!(identity.serial_number, Some(0x304a_5a55));
        assert_eq!(identity.serial_text.as_deref(), Some("7JQXBY2"));
        assert_eq!(
            identity.manufacture,
            PanelManufacture {
                year: 2019,
                week: Some(36),
                model_year: false
            }
        );
        assert_eq!(
            identity.native_resolution,
            Some(PanelResolution {
                width: 2560,
                height: 1440
            })
        );
        assert_eq!(identity.native_refresh_hz, Some(144));
        assert_eq!(
            identity.limits,
            Some(PanelRangeLimits {
                min_refresh_hz: 40,
                max_refresh_hz: 155,
                min_horizontal_khz: 250,
                max_horizontal_khz: 250,
                max_pixel_clock_mhz: 640
            })
        );
        assert_eq!(
            identity.input,
            PanelInput {
                digital: true,
                bit_depth: Some(8),
                interface_code: 5
            }
        );
        assert_eq!(identity.connection, PanelConnection::DisplayPort);
        assert_eq!(identity.extension_blocks, 0);
    }

    #[test]
    fn refuses_a_value_that_is_not_whole_blocks() {
        // A registry value truncated mid-block: the length check runs before
        // any field is read, so no partial identity is ever reported.
        let mut value = bytes(REFERENCE_EDID);
        value.truncate(BLOCK_LEN + 3);
        let error = decode_panel_edid(&value, true).expect_err("131-byte value");
        assert!(
            error.to_string().contains("131 bytes"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn refuses_a_block_with_a_broken_checksum() {
        // One flipped byte without a reseal: the block no longer sums to zero.
        let mut value = bytes(REFERENCE_EDID);
        value[100] ^= 0xff;
        let error = decode_panel_edid(&value, true).expect_err("broken checksum");
        assert!(
            error.to_string().contains("block 0 fails its checksum"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn refuses_a_missing_extension_block() {
        let mut block = bytes(REFERENCE_EDID);
        block.truncate(2 * BLOCK_LEN);
        let error = decode_panel_edid(&block, true).expect_err("declared block missing");
        assert!(
            error.to_string().contains("declares 3 block(s) but 2 were read"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn refuses_a_foreign_header() {
        let mut value = bytes(REFERENCE_EDID);
        value[7] = 0x01;
        // Keep the block checksum valid so the header check is what fails.
        reseal(&mut value, 0);
        let error = decode_panel_edid(&value, true).expect_err("foreign header");
        assert!(
            error.to_string().contains("EDID magic"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn refuses_an_incomplete_preferred_timing() {
        let mut value = bytes(REFERENCE_EDID);
        // Zero the vertical active field of the first detailed timing.
        value[54 + 5] = 0;
        value[54 + 7] &= 0x0f;
        reseal(&mut value, 0);
        let error = decode_panel_edid(&value, true).expect_err("incomplete timing");
        assert!(
            error.to_string().contains("not a complete timing"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn an_analog_panel_has_no_interface_code() {
        let mut value = bytes(REFERENCE_EDID);
        value[20] = 0x00;
        reseal(&mut value, 0);
        let identity = decode_panel_edid(&value, true).expect("analog EDID");
        assert_eq!(identity.connection, PanelConnection::Analog);
        assert_eq!(
            identity.input,
            PanelInput {
                digital: false,
                bit_depth: None,
                interface_code: 0
            }
        );
    }

    #[test]
    fn decodes_the_reference_panel_hdr_block() {
        // The reference panel's CTA extension carries
        //   e3 05 80 80           Colorimetry: BT.2020 RGB
        //   e6 06 05 01 6a 6a 48  HDR static metadata: EOTF 0x05, SMD 0x01,
        //                         max/avg code 106, min code 72
        let capability = decode_panel_hdr_capability(&bytes(REFERENCE_EDID))
            .expect("reference EDID")
            .expect("reference panel declares HDR");
        assert!(capability.eotf_sdr);
        assert!(
            capability.eotf_hdr10_pq,
            "EOTF bit 2 is SMPTE ST 2084, i.e. HDR10"
        );
        assert!(!capability.eotf_hlg, "EOTF bit 3 is not set");
        assert!(capability.bt2020_rgb);
        assert!(capability.static_metadata_type1);
        assert_eq!(capability.max_luminance_code, Some(106));
        assert_eq!(capability.average_luminance_code, Some(106));
        assert_eq!(capability.min_luminance_code, Some(72));
        assert_eq!(capability.max_luminance_millinits, Some(496_743));
        assert_eq!(capability.average_luminance_millinits, Some(496_743));
        assert_eq!(capability.min_luminance_millinits, Some(396));
    }

    #[test]
    fn a_sink_without_a_cta_extension_declares_no_hdr() {
        // Base block only: an EDID that declares zero extension blocks is valid
        // and carries no capability block at all.
        let mut value = bytes(REFERENCE_EDID);
        value[EXTENSION_COUNT_OFFSET] = 0;
        reseal(&mut value, 0);
        value.truncate(BLOCK_LEN);

        assert_eq!(
            decode_panel_hdr_capability(&value).expect("valid base block"),
            None
        );
    }

    #[test]
    fn a_partial_luminance_triple_is_refused() {
        // Shorten the HDR block from 6 payload bytes to 5 so it declares only
        // two of the three luminance codes, then reseal the extension block.
        let mut value = bytes(REFERENCE_EDID);
        value[BLOCK_LEN + 8] = 0xe5;
        reseal(&mut value, 1);

        let error = decode_panel_hdr_capability(&value).expect_err("partial luminance triple");
        assert!(
            error
                .to_string()
                .contains("luminance triple is all or nothing"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn a_data_block_that_overruns_the_collection_is_refused() {
        // Claim a longer payload than the collection can hold: the parser must
        // not read timing bytes as capability bits.
        let mut value = bytes(REFERENCE_EDID);
        value[BLOCK_LEN + 4] = 0xe7;
        reseal(&mut value, 1);

        let error = decode_panel_hdr_capability(&value).expect_err("overrunning block");
        assert!(
            error.to_string().contains("past the data block collection"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn luminance_codes_follow_the_cta_curve() {
        assert_eq!(luminance_millinits(0), 50_000);
        assert_eq!(luminance_millinits(32), 100_000);
        assert_eq!(luminance_millinits(255), 12_525_720);
        // The minimum is relative to the decoded maximum, not to 50 nits.
        assert_eq!(min_luminance_millinits(496_743, 0), 0);
        assert_eq!(min_luminance_millinits(496_743, 255), 4_967);
    }
}
