use crate::model::*;
use std::io::Cursor;

fn malformed(message: impl Into<String>) -> Error {
    Error::new(ErrorCode::Malformed, message)
}
fn unsupported(message: &str) -> Error {
    Error::new(ErrorCode::Unsupported, message)
}
fn png_error(error: png::DecodingError) -> Error {
    Error::new(
        if matches!(error, png::DecodingError::LimitsExceeded) {
            ErrorCode::ResourceLimit
        } else {
            ErrorCode::Malformed
        },
        format!("embedded PNG: {error}"),
    )
}

fn jpeg_extent(bytes: &[u8], limits: &Limits) -> Result<()> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return Err(malformed("invalid embedded JPEG signature"));
    }
    let mut at = 2usize;
    let mut segments = 0;
    loop {
        if bytes.get(at) != Some(&0xff) {
            return Err(malformed("expected JPEG marker"));
        }
        while bytes.get(at) == Some(&0xff) {
            at += 1;
        }
        let marker = *bytes
            .get(at)
            .ok_or_else(|| malformed("truncated JPEG marker"))?;
        at += 1;
        segments += 1;
        if segments > limits.max_operations {
            return Err(Error::new(
                ErrorCode::ResourceLimit,
                "JPEG marker count limit exceeded",
            ));
        }
        if marker == 0xd9 {
            return if at == bytes.len() {
                Ok(())
            } else {
                Err(malformed("trailing data after embedded JPEG"))
            };
        }
        if marker == 0xd8 || marker == 0 || (0xd0..=0xd7).contains(&marker) {
            return Err(malformed("unexpected JPEG marker"));
        }
        if marker == 1 {
            continue;
        }
        let length = bytes
            .get(at..at + 2)
            .ok_or_else(|| malformed("truncated JPEG segment length"))?;
        let len = u16::from_be_bytes([length[0], length[1]]) as usize;
        if len < 2 {
            return Err(malformed("invalid JPEG segment length"));
        }
        at = at
            .checked_add(len)
            .filter(|&n| n <= bytes.len())
            .ok_or_else(|| malformed("JPEG segment outside payload"))?;
        if marker == 0xda {
            loop {
                let byte = *bytes
                    .get(at)
                    .ok_or_else(|| malformed("unterminated JPEG scan"))?;
                if byte == 0xff {
                    let mut end = at + 1;
                    while bytes.get(end) == Some(&0xff) {
                        end += 1;
                    }
                    let next = *bytes
                        .get(end)
                        .ok_or_else(|| malformed("truncated JPEG entropy marker"))?;
                    if next != 0 && !(0xd0..=0xd7).contains(&next) {
                        break;
                    }
                    at = end + 1;
                } else {
                    at += 1;
                }
            }
        }
    }
}

pub(crate) struct Decoded {
    pub rgb: Vec<u8>,
    pub alpha: Option<Vec<u8>>,
}

fn direct_pixel(bytes: &[u8], depth: u16, raw: bool) -> Result<([u8; 3], u8)> {
    Ok(match depth {
        16 => {
            let value = u16::from_be_bytes([bytes[0], bytes[1]]);
            if raw && value & 0x8000 != 0 {
                return Err(unsupported("QuickTime RGB555 reserved high bit is set"));
            }
            let expand = |shift: u32| {
                let v = ((value >> shift) & 31u16) as u8;
                (v << 3) | (v >> 2)
            };
            ([expand(10), expand(5), expand(0)], 255)
        }
        24 => ([bytes[0], bytes[1], bytes[2]], 255),
        32 => ([bytes[1], bytes[2], bytes[3]], bytes[0]),
        _ => unreachable!("validated direct depth"),
    })
}

fn direct(
    codec: &[u8],
    bytes: &[u8],
    width: usize,
    height: usize,
    depth: u16,
    budget: &mut Budget<'_>,
) -> Result<Decoded> {
    if !matches!(depth, 16 | 24 | 32) {
        return Err(unsupported(
            "QuickTime raw/Animation supports only packed RGB555, RGB24 and ARGB32; palettes/grayscale are unsupported",
        ));
    }
    let step = usize::from(depth / 8);
    let pixels = width * height;
    budget.allocate(pixels * 4)?;
    let mut image = Decoded {
        rgb: vec![0; pixels * 3],
        alpha: (depth == 32).then(|| vec![0; pixels]),
    };
    let mut put = |index: usize, bytes: &[u8]| -> Result<()> {
        let (rgb, alpha) = direct_pixel(bytes, depth, codec == b"raw ")?;
        image.rgb[index * 3..index * 3 + 3].copy_from_slice(&rgb);
        if let Some(values) = &mut image.alpha {
            values[index] = alpha;
        }
        Ok(())
    };
    if codec == b"raw " {
        let stride = (width * step + 1) & !1;
        if bytes.len() != stride * height {
            return Err(malformed(
                "QuickTime raw length must equal even-byte-padded scanlines",
            ));
        }
        for y in 0..height {
            for x in 0..width {
                let at = y * stride + x * step;
                put(y * width + x, &bytes[at..at + step])?;
            }
        }
        return Ok(image);
    }
    let mut r = RleReader { bytes, at: 0 };
    let size = u32::from_be_bytes(r.take(4)?.try_into().expect("four bytes"));
    if size & 0xc0000000 != 0 {
        return Err(unsupported("Animation chunk size flags are unsupported"));
    }
    if size as usize != bytes.len() {
        return Err(malformed("Animation chunk length mismatch"));
    }
    if size < 8 {
        return Err(unsupported(
            "Animation unchanged/delta frames require unavailable previous pixels",
        ));
    }
    match r.word()? {
        0 => {}
        8 => {
            let start = r.word()?;
            let reserved = r.word()?;
            let lines = r.word()?;
            let reserved2 = r.word()?;
            if reserved != 0 || reserved2 != 0 {
                return Err(unsupported(
                    "Animation reserved update-header fields are nonzero",
                ));
            }
            if start != 0 || usize::from(lines) != height {
                return Err(unsupported(
                    "Animation partial-frame updates require a previous frame",
                ));
            }
        }
        _ => return Err(unsupported("Animation chunk header flags are unsupported")),
    }
    let mut commands = 0usize;
    for y in 0..height {
        if r.byte()? != 1 {
            return Err(unsupported(
                "Animation skipped pixels/rows require a previous frame",
            ));
        }
        let mut x = 0usize;
        loop {
            commands += 1;
            if commands > budget.limits.max_operations {
                return Err(Error::new(
                    ErrorCode::ResourceLimit,
                    "Animation command count limit exceeded",
                ));
            }
            budget.work(1)?;
            let code = r.byte()? as i8;
            if code == -1 {
                if x != width {
                    return Err(unsupported(
                        "Animation incomplete row requires a previous frame",
                    ));
                }
                break;
            }
            if code == 0 {
                if r.byte()? != 1 {
                    return Err(unsupported(
                        "Animation pixel skips require a previous frame",
                    ));
                }
                continue;
            }
            let count = usize::from(code.unsigned_abs());
            if x + count > width {
                return Err(malformed("Animation run exceeds scanline"));
            }
            if code > 0 {
                for pixel in r.take(count * step)?.chunks_exact(step) {
                    put(y * width + x, pixel)?;
                    x += 1;
                }
            } else {
                let pixel = r.take(step)?;
                for _ in 0..count {
                    put(y * width + x, pixel)?;
                    x += 1;
                }
            }
        }
    }
    if r.at < bytes.len() && r.byte()? != 0 {
        return Err(malformed("invalid Animation frame terminator"));
    }
    if r.at != bytes.len() {
        return Err(malformed("trailing data after Animation still frame"));
    }
    Ok(image)
}

struct RleReader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> RleReader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(len)
            .filter(|&n| n <= self.bytes.len())
            .ok_or_else(|| malformed("truncated Animation data"))?;
        let result = &self.bytes[self.at..end];
        self.at = end;
        Ok(result)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn word(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }
}

pub(crate) fn decode(
    codec: &[u8],
    bytes: &[u8],
    width: usize,
    height: usize,
    depth: u16,
    budget: &mut Budget<'_>,
) -> Result<Decoded> {
    let pixels = budget.limits.pixels(width, height)?;
    budget.work(pixels as u64 * 4 + bytes.len() as u64)?;
    match codec {
        b"raw " | b"rle " => direct(codec, bytes, width, height, depth, budget),
        b"png " => {
            if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Err(malformed("invalid embedded PNG signature"));
            }
            let mut at = 8usize;
            let mut chunks = 0;
            loop {
                let header = bytes
                    .get(at..at + 8)
                    .ok_or_else(|| malformed("truncated embedded PNG chunk"))?;
                let len =
                    u32::from_be_bytes(header[..4].try_into().expect("chunk length")) as usize;
                let end = at
                    .checked_add(12)
                    .and_then(|n| n.checked_add(len))
                    .filter(|&n| n <= bytes.len())
                    .ok_or_else(|| malformed("embedded PNG chunk exceeds payload"))?;
                chunks += 1;
                if chunks > budget.limits.max_operations {
                    return Err(Error::new(
                        ErrorCode::ResourceLimit,
                        "PNG chunk count limit exceeded",
                    ));
                }
                if &header[4..] == b"acTL" {
                    return Err(unsupported(
                        "animated PNG in QuickTime is not a still image",
                    ));
                }
                if &header[4..] == b"IEND" {
                    if len != 0 || end != bytes.len() {
                        return Err(malformed("invalid PNG ending or trailing image data"));
                    }
                    break;
                }
                at = end;
            }
            let mut decoder = png::Decoder::new_with_limits(
                Cursor::new(bytes),
                png::Limits {
                    bytes: budget.limits.max_decoded_bytes,
                },
            );
            decoder.set_transformations(png::Transformations::EXPAND);
            let mut reader = decoder.read_info().map_err(png_error)?;
            if (reader.info().width as usize, reader.info().height as usize) != (width, height) {
                return Err(malformed(
                    "PNG dimensions differ from QuickTime image description",
                ));
            }
            if reader.info().bit_depth == png::BitDepth::Sixteen {
                return Err(unsupported("16-bit embedded PNG is not implemented"));
            }
            let size = reader.output_buffer_size();
            budget.allocate(size)?;
            let mut buffer = vec![0; size];
            let info = reader.next_frame(&mut buffer).map_err(png_error)?;
            reader.finish().map_err(png_error)?;
            budget.allocate(pixels * 4)?;
            let mut rgb = Vec::with_capacity(pixels * 3);
            let mut alpha = Vec::with_capacity(pixels);
            for pixel in buffer[..info.buffer_size()].chunks_exact(info.color_type.samples()) {
                match info.color_type {
                    png::ColorType::Rgb => rgb.extend_from_slice(pixel),
                    png::ColorType::Grayscale => rgb.extend_from_slice(&[pixel[0]; 3]),
                    png::ColorType::Rgba | png::ColorType::GrayscaleAlpha => {
                        alpha.push(pixel[pixel.len() - 1]);
                        if pixel.len() == 2 {
                            rgb.extend_from_slice(&[pixel[0]; 3]);
                        } else {
                            rgb.extend_from_slice(&pixel[..3]);
                        }
                    }
                    _ => return Err(unsupported("embedded PNG sample layout not implemented")),
                }
            }
            Ok(Decoded {
                rgb,
                alpha: (!alpha.is_empty()).then_some(alpha),
            })
        }
        b"jpeg" => {
            jpeg_extent(bytes, budget.limits)?;
            let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
            decoder.set_max_decoding_buffer_size(budget.limits.max_decoded_bytes);
            decoder
                .read_info()
                .map_err(|e| malformed(format!("embedded JPEG header: {e}")))?;
            let info = decoder
                .info()
                .ok_or_else(|| malformed("missing JPEG dimensions"))?;
            if (info.width as usize, info.height as usize) != (width, height) {
                return Err(malformed(
                    "JPEG dimensions differ from QuickTime image description",
                ));
            }
            if !matches!(
                info.pixel_format,
                jpeg_decoder::PixelFormat::L8 | jpeg_decoder::PixelFormat::RGB24
            ) {
                return Err(unsupported(
                    "embedded JPEG only supports 8-bit grayscale/RGB, not CMYK or 16-bit samples",
                ));
            }
            budget.allocate(pixels * info.pixel_format.pixel_bytes())?;
            let decoded = decoder
                .decode()
                .map_err(|e| malformed(format!("embedded JPEG: {e}")))?;
            if info.pixel_format == jpeg_decoder::PixelFormat::RGB24 {
                return Ok(Decoded {
                    rgb: decoded,
                    alpha: None,
                });
            }
            budget.allocate(pixels * 3)?;
            Ok(Decoded {
                rgb: decoded.into_iter().flat_map(|v| [v; 3]).collect(),
                alpha: None,
            })
        }
        _ => Err(Error::new(
            ErrorCode::Unsupported,
            format!(
                "embedded QuickTime codec {codec:02x?} is not implemented (supported: jpeg, png , raw , rle )"
            ),
        )),
    }
}
