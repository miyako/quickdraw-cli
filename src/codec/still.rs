use super::{Decoded, malformed, unsupported};
use crate::model::*;
use std::{collections::BTreeMap, io::Cursor, num::NonZeroU64};

fn limit(message: &str) -> Error {
    Error::new(ErrorCode::ResourceLimit, message)
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&v| v <= self.data.len())
            .ok_or_else(|| malformed("truncated embedded still-image structure"))?;
        let result = &self.data[self.at..end];
        self.at = end;
        Ok(result)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn word(&mut self) -> Result<usize> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()) as usize)
    }
}
fn operation(count: &mut usize, budget: &mut Budget<'_>) -> Result<()> {
    *count += 1;
    if *count > budget.limits.max_operations {
        return Err(limit("still-image structure count exceeded"));
    }
    budget.work(1)
}
fn subblocks(r: &mut Reader<'_>, count: &mut usize, budget: &mut Budget<'_>) -> Result<()> {
    loop {
        operation(count, budget)?;
        let n = r.byte()? as usize;
        if n == 0 {
            return Ok(());
        }
        r.take(n)?;
    }
}

pub(super) fn gif(
    bytes: &[u8],
    width: usize,
    height: usize,
    budget: &mut Budget<'_>,
) -> Result<Decoded> {
    let mut r = Reader { data: bytes, at: 0 };
    let signature = r.take(6)?;
    if signature != b"GIF87a" && signature != b"GIF89a" {
        return Err(malformed("invalid GIF signature"));
    }
    if (r.word()?, r.word()?) != (width, height) {
        return Err(malformed(
            "GIF logical dimensions differ from QuickTime description",
        ));
    }
    let packed = r.byte()?;
    r.byte()?;
    if !matches!(r.byte()?, 0 | 49) {
        return Err(unsupported("GIF non-square pixel aspect is unsupported"));
    }
    let global = if packed & 128 != 0 {
        r.take(3 * (1usize << ((packed & 7) + 1)))?
    } else {
        &[]
    };
    let mut palette = global;
    let mut transparent = None;
    let mut gce = false;
    let mut frames = 0;
    let mut count = 0;
    loop {
        operation(&mut count, budget)?;
        match r.byte()? {
            0x3b => {
                if frames != 1 || r.at != bytes.len() {
                    return Err(malformed(
                        "GIF requires exactly one image and a final trailer",
                    ));
                }
                break;
            }
            0x21 => match r.byte()? {
                0xf9 => {
                    if signature != b"GIF89a" || gce || frames != 0 {
                        return Err(unsupported(
                            "GIF has duplicate, misplaced or version-incompatible control",
                        ));
                    }
                    gce = true;
                    if r.byte()? != 4 {
                        return Err(malformed("invalid GIF graphics-control length"));
                    }
                    let flags = r.byte()?;
                    if flags & 0xe0 != 0 {
                        return Err(malformed("reserved GIF graphics-control bits"));
                    }
                    if flags & 2 != 0 || (flags >> 2) & 7 > 1 {
                        return Err(unsupported(
                            "GIF user input or disposal-dependent image is unsupported",
                        ));
                    }
                    r.take(2)?;
                    let index = r.byte()?;
                    if flags & 1 != 0 {
                        transparent = Some(index);
                    }
                    if r.byte()? != 0 {
                        return Err(malformed("GIF graphics control is not terminated"));
                    }
                }
                0xfe => subblocks(&mut r, &mut count, budget)?,
                _ => {
                    return Err(unsupported(
                        "GIF application/color-profile, plain-text or unknown extension is unsupported",
                    ));
                }
            },
            0x2c => {
                frames += 1;
                if frames != 1 {
                    return Err(unsupported(
                        "animated/multi-image GIF is not a single still",
                    ));
                }
                if (r.word()?, r.word()?, r.word()?, r.word()?) != (0, 0, width, height) {
                    return Err(unsupported(
                        "GIF image must cover its entire logical screen without prior pixels",
                    ));
                }
                let flags = r.byte()?;
                if flags & 0x18 != 0 {
                    return Err(malformed("reserved GIF image-descriptor bits"));
                }
                if flags & 128 != 0 {
                    palette = r.take(3 * (1usize << ((flags & 7) + 1)))?;
                }
                if palette.is_empty() {
                    return Err(unsupported("GIF has no explicit color table"));
                }
                if transparent.is_some_and(|i| usize::from(i) >= palette.len() / 3) {
                    return Err(malformed("GIF transparency index is outside the palette"));
                }
                if !(2..=8).contains(&r.byte()?) {
                    return Err(malformed("invalid GIF LZW minimum code size"));
                }
                subblocks(&mut r, &mut count, budget)?;
            }
            _ => return Err(malformed("invalid GIF block")),
        }
    }
    let pixels = width * height;
    budget.allocate(
        pixels
            .checked_mul(5)
            .and_then(|n| n.checked_add(256 * 1024))
            .ok_or_else(|| limit("GIF allocation overflow"))?,
    )?;
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::Indexed);
    options.set_memory_limit(gif::MemoryLimit::Bytes(
        NonZeroU64::new(pixels as u64).unwrap(),
    ));
    options.check_frame_consistency(true);
    options.check_lzw_end_code(true);
    let mut decoder = options
        .read_info(Cursor::new(bytes))
        .map_err(|e| malformed(format!("embedded GIF: {e}")))?;
    let frame = decoder
        .read_next_frame()
        .map_err(|e| malformed(format!("embedded GIF: {e}")))?
        .ok_or_else(|| malformed("missing GIF frame"))?;
    if frame.buffer.len() != pixels {
        return Err(malformed("GIF decoded sample count mismatch"));
    }
    let mut rgb = Vec::with_capacity(pixels * 3);
    let mut alpha = transparent.map(|_| Vec::with_capacity(pixels));
    for &index in frame.buffer.iter() {
        let at = usize::from(index) * 3;
        rgb.extend_from_slice(
            palette
                .get(at..at + 3)
                .ok_or_else(|| malformed("GIF pixel index outside palette"))?,
        );
        if let Some(a) = &mut alpha {
            a.push(if Some(index) == transparent { 0 } else { 255 });
        }
    }
    if decoder
        .read_next_frame()
        .map_err(|e| malformed(format!("embedded GIF ending: {e}")))?
        .is_some()
    {
        return Err(unsupported("multiple GIF frames"));
    }
    Ok(Decoded { rgb, alpha })
}

struct Tiff<'a> {
    data: &'a [u8],
    little: bool,
}
impl<'a> Tiff<'a> {
    fn bytes(&self, at: usize, n: usize) -> Result<&'a [u8]> {
        let end = at
            .checked_add(n)
            .ok_or_else(|| malformed("TIFF extent overflow"))?;
        self.data
            .get(at..end)
            .ok_or_else(|| malformed("TIFF extent outside payload"))
    }
    fn short(&self, at: usize) -> Result<u16> {
        let v = self.bytes(at, 2)?.try_into().unwrap();
        Ok(if self.little {
            u16::from_le_bytes(v)
        } else {
            u16::from_be_bytes(v)
        })
    }
    fn long(&self, at: usize) -> Result<u32> {
        let v = self.bytes(at, 4)?.try_into().unwrap();
        Ok(if self.little {
            u32::from_le_bytes(v)
        } else {
            u32::from_be_bytes(v)
        })
    }
}
struct Tag {
    kind: u16,
    count: usize,
    at: usize,
}
fn values(t: &Tiff<'_>, tags: &BTreeMap<u16, Tag>, id: u16) -> Result<Vec<u32>> {
    let Some(tag) = tags.get(&id) else {
        return Ok(Vec::new());
    };
    if !matches!(tag.kind, 3 | 4) {
        return Err(malformed(format!("TIFF tag {id} requires SHORT/LONG")));
    }
    (0..tag.count)
        .map(|i| {
            if tag.kind == 3 {
                t.short(tag.at + i * 2).map(u32::from)
            } else {
                t.long(tag.at + i * 4)
            }
        })
        .collect()
}
fn scalar(t: &Tiff<'_>, tags: &BTreeMap<u16, Tag>, id: u16, default: u32) -> Result<u32> {
    let v = values(t, tags, id)?;
    match v.as_slice() {
        [] => Ok(default),
        [n] => Ok(*n),
        _ => Err(malformed(format!("TIFF tag {id} is not scalar"))),
    }
}
fn tiff_error(error: tiff::TiffError) -> Error {
    Error::new(
        match error {
            tiff::TiffError::LimitsExceeded => ErrorCode::ResourceLimit,
            tiff::TiffError::UnsupportedError(_) => ErrorCode::Unsupported,
            _ => ErrorCode::Malformed,
        },
        format!("embedded TIFF: {error}"),
    )
}

fn strip_extent(
    bytes: &[u8],
    expected: usize,
    compression: u32,
    budget: &mut Budget<'_>,
) -> Result<()> {
    match compression {
        1 => {
            if bytes.len() != expected {
                return Err(malformed("uncompressed TIFF strip length mismatch"));
            }
        }
        32773 => {
            let mut r = Reader { data: bytes, at: 0 };
            let mut produced = 0usize;
            while r.at < bytes.len() {
                let code = r.byte()? as i8;
                let count = if code >= 0 {
                    let count = code as usize + 1;
                    r.take(count)?;
                    count
                } else if code != -128 {
                    r.byte()?;
                    (1 - i16::from(code)) as usize
                } else {
                    0
                };
                produced = produced
                    .checked_add(count)
                    .filter(|&v| v <= expected)
                    .ok_or_else(|| malformed("TIFF PackBits expands beyond its strip"))?;
            }
            if produced != expected {
                return Err(malformed("TIFF PackBits strip is incomplete"));
            }
        }
        8 | 32946 => {
            let mut output = vec![0; expected + 1];
            let mut decoder = flate2::Decompress::new(true);
            let status = decoder
                .decompress(bytes, &mut output, flate2::FlushDecompress::Finish)
                .map_err(|e| malformed(format!("TIFF Deflate stream: {e}")))?;
            if status != flate2::Status::StreamEnd
                || decoder.total_out() != expected as u64
                || decoder.total_in() != bytes.len() as u64
            {
                return Err(malformed(
                    "TIFF Deflate strip has incomplete, excessive or trailing data",
                ));
            }
        }
        5 => {
            let mut output = vec![0; expected + 1];
            let mut decoder =
                weezl::decode::Configuration::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
                    .with_yield_on_full_buffer(true)
                    .build();
            let (mut input, mut produced) = (0, 0);
            loop {
                budget.work(1)?;
                let result = decoder.decode_bytes(&bytes[input..], &mut output[produced..]);
                input += result.consumed_in;
                produced += result.consumed_out;
                if produced > expected {
                    return Err(malformed("TIFF LZW expands beyond its strip"));
                }
                match result.status {
                    Ok(weezl::LzwStatus::Done) => {
                        if produced != expected || input != bytes.len() {
                            return Err(malformed("TIFF LZW strip length/ending mismatch"));
                        }
                        break;
                    }
                    Ok(weezl::LzwStatus::Ok) if result.consumed_in + result.consumed_out > 0 => {}
                    _ => return Err(malformed("TIFF LZW strip is invalid or lacks its end code")),
                }
            }
        }
        _ => unreachable!("validated TIFF compression"),
    }
    Ok(())
}

pub(super) fn tiff(
    bytes: &[u8],
    width: usize,
    height: usize,
    budget: &mut Budget<'_>,
) -> Result<Decoded> {
    let little = match bytes.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return Err(malformed("invalid TIFF byte order")),
    };
    let t = Tiff {
        data: bytes,
        little,
    };
    match t.short(2)? {
        42 => {}
        43 => return Err(unsupported("BigTIFF is outside the single-page profile")),
        _ => return Err(malformed("invalid TIFF version")),
    }
    let at = t.long(4)? as usize;
    if at < 8 {
        return Err(malformed("invalid TIFF first IFD offset"));
    }
    let count = t.short(at)? as usize;
    if count > budget.limits.max_operations {
        return Err(limit("TIFF IFD entry count exceeded"));
    }
    t.bytes(at + 2, count * 12 + 4)?;
    if t.long(at + 2 + count * 12)? != 0 {
        return Err(unsupported(
            "TIFF has another page/IFD; no implicit page selection",
        ));
    }
    budget.allocate(count * 128)?;
    budget.work(count as u64)?;
    let mut tags = BTreeMap::new();
    let mut previous = 0;
    for i in 0..count {
        let entry = at + 2 + i * 12;
        let id = t.short(entry)?;
        if id <= previous {
            return Err(malformed("TIFF tags must be unique and sorted"));
        }
        previous = id;
        let kind = t.short(entry + 2)?;
        let size = match kind {
            1 | 2 => 1,
            3 => 2,
            4 => 4,
            5 => 8,
            _ => {
                return Err(unsupported(
                    "TIFF field type outside baseline unsigned profile",
                ));
            }
        };
        let n = t.long(entry + 4)? as usize;
        if n == 0 {
            return Err(malformed("empty TIFF field"));
        }
        let len = n
            .checked_mul(size)
            .ok_or_else(|| malformed("TIFF field size overflow"))?;
        let data_at = if len <= 4 {
            entry + 8
        } else {
            t.long(entry + 8)? as usize
        };
        let data = t.bytes(data_at, len)?;
        budget.allocate(
            len.checked_mul(8)
                .ok_or_else(|| limit("TIFF metadata allocation overflow"))?,
        )?;
        budget.work(n as u64)?;
        match id {
            254..=259 | 262 | 266 | 273 | 274 | 277..=279 | 284 | 296 | 297 | 317 | 338 | 339 => {}
            269..=272 | 285 | 305 | 306 | 315 | 316 | 33432 => {
                if kind != 2 || data.last() != Some(&0) {
                    return Err(malformed("TIFF text metadata must be terminated ASCII"));
                }
            }
            282 | 283 => {
                if kind != 5 || n != 1 || t.long(data_at)? == 0 || t.long(data_at + 4)? == 0 {
                    return Err(malformed("invalid TIFF resolution rational"));
                }
            }
            _ => {
                return Err(unsupported(
                    "TIFF palette, color-management, subimage, tile or unknown semantic tag is unsupported",
                ));
            }
        }
        tags.insert(
            id,
            Tag {
                kind,
                count: n,
                at: data_at,
            },
        );
    }
    if (
        scalar(&t, &tags, 256, 0)? as usize,
        scalar(&t, &tags, 257, 0)? as usize,
    ) != (width, height)
    {
        return Err(malformed(
            "TIFF dimensions differ from QuickTime description",
        ));
    }
    for (id, default) in [(254, 0), (255, 1), (266, 1), (274, 1), (284, 1)] {
        if scalar(&t, &tags, id, default)? != default {
            return Err(unsupported(
                "TIFF subimage, fill order, orientation or planar configuration is unsupported",
            ));
        }
    }
    let page = values(&t, &tags, 297)?;
    if !page.is_empty() && page != [0, 1] {
        return Err(unsupported("TIFF page number does not declare one page"));
    }
    let photo = scalar(&t, &tags, 262, u32::MAX)?;
    let samples = scalar(&t, &tags, 277, 1)? as usize;
    if !matches!((photo, samples), (0 | 1, 1) | (2, 3 | 4)) {
        return Err(unsupported(
            "TIFF requires 8-bit gray, RGB or straight RGBA; palette/CMYK/Lab are unsupported",
        ));
    }
    let bits = values(&t, &tags, 258)?;
    if bits.len() != samples || bits.iter().any(|&b| b != 8) {
        return Err(unsupported(
            "TIFF requires explicit 8-bit unsigned components",
        ));
    }
    let formats = values(&t, &tags, 339)?;
    if !formats.is_empty() && (formats.len() != samples || formats.iter().any(|&v| v != 1)) {
        return Err(unsupported("TIFF sample format must be unsigned"));
    }
    let extras = values(&t, &tags, 338)?;
    if (samples == 4 && extras != [2]) || (samples != 4 && !extras.is_empty()) {
        return Err(unsupported(
            "TIFF alpha must be explicitly unassociated; extra channels are unsupported",
        ));
    }
    let compression = scalar(&t, &tags, 259, 1)?;
    if !matches!(compression, 1 | 5 | 8 | 32946 | 32773) {
        return Err(unsupported(
            "TIFF compression must be none, LZW, Deflate or PackBits",
        ));
    }
    if !matches!(scalar(&t, &tags, 317, 1)?, 1 | 2) {
        return Err(unsupported("TIFF predictor must be none or horizontal"));
    }
    if !matches!(scalar(&t, &tags, 296, 2)?, 1..=3) {
        return Err(malformed("invalid TIFF resolution unit"));
    }
    let rows = scalar(&t, &tags, 278, height as u32)? as usize;
    if rows == 0 {
        return Err(malformed("zero TIFF rows per strip"));
    }
    let offsets = values(&t, &tags, 273)?;
    let lengths = values(&t, &tags, 279)?;
    if offsets.len() != height.div_ceil(rows) || offsets.len() != lengths.len() {
        return Err(malformed("TIFF strip count mismatch"));
    }
    if offsets.len() > budget.limits.max_operations {
        return Err(limit("TIFF strip count limit exceeded"));
    }
    let pixels = width * height;
    let decoded = pixels * samples;
    budget.allocate(decoded * 2 + pixels * 4 + 256 * 1024 + offsets.len())?;
    budget.work(decoded as u64 * 4)?;
    for (i, (&offset, &length)) in offsets.iter().zip(&lengths).enumerate() {
        if length == 0 {
            return Err(malformed("empty TIFF strip"));
        }
        let bytes = t.bytes(offset as usize, length as usize)?;
        budget.work(u64::from(length) * 2)?;
        budget.allocate(length as usize * 2)?;
        strip_extent(
            bytes,
            width * samples * (height - i * rows).min(rows),
            compression,
            budget,
        )?;
    }
    let mut limits = tiff::decoder::Limits::default();
    limits.decoding_buffer_size = decoded;
    limits.intermediate_buffer_size = budget.limits.max_decoded_bytes;
    limits.ifd_value_size = budget.limits.max_decoded_bytes;
    let mut decoder = tiff::decoder::Decoder::new(Cursor::new(bytes))
        .map_err(tiff_error)?
        .with_limits(limits);
    if decoder.more_images() {
        return Err(unsupported("multiple TIFF images"));
    }
    let tiff::decoder::DecodingResult::U8(buffer) = decoder.read_image().map_err(tiff_error)?
    else {
        return Err(unsupported(
            "TIFF decoder returned a non-byte sample layout",
        ));
    };
    if buffer.len() != decoded {
        return Err(malformed("TIFF decoded sample count mismatch"));
    }
    let mut rgb = Vec::with_capacity(pixels * 3);
    let mut alpha = (samples == 4).then(|| Vec::with_capacity(pixels));
    for p in buffer.chunks_exact(samples) {
        if samples == 1 {
            rgb.extend([p[0]; 3]);
        } else {
            rgb.extend_from_slice(&p[..3]);
        }
        if let Some(a) = &mut alpha {
            a.push(p[3]);
        }
    }
    Ok(Decoded { rgb, alpha })
}
