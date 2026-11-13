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

pub(crate) fn decode(
    codec: &[u8],
    bytes: &[u8],
    width: usize,
    height: usize,
    budget: &mut Budget<'_>,
) -> Result<Vec<u8>> {
    let pixels = budget.limits.pixels(width, height)?;
    budget.work(pixels as u64 * 4 + bytes.len() as u64)?;
    match codec {
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
            budget.allocate(pixels * 3)?;
            let mut rgb = Vec::with_capacity(pixels * 3);
            for pixel in buffer[..info.buffer_size()].chunks_exact(info.color_type.samples()) {
                match info.color_type {
                    png::ColorType::Rgb => rgb.extend_from_slice(pixel),
                    png::ColorType::Grayscale => rgb.extend_from_slice(&[pixel[0]; 3]),
                    png::ColorType::Rgba | png::ColorType::GrayscaleAlpha => {
                        if pixel[pixel.len() - 1] != 255 {
                            return Err(unsupported(
                                "nonopaque embedded PNG requires unimplemented alpha compositing",
                            ));
                        }
                        if pixel.len() == 2 {
                            rgb.extend_from_slice(&[pixel[0]; 3]);
                        } else {
                            rgb.extend_from_slice(&pixel[..3]);
                        }
                    }
                    _ => return Err(unsupported("embedded PNG sample layout not implemented")),
                }
            }
            Ok(rgb)
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
                return Ok(decoded);
            }
            budget.allocate(pixels * 3)?;
            Ok(decoded.into_iter().flat_map(|v| [v; 3]).collect())
        }
        _ => Err(Error::new(
            ErrorCode::Unsupported,
            format!(
                "embedded QuickTime codec {codec:02x?} is not implemented (supported: jpeg, png )"
            ),
        )),
    }
}
