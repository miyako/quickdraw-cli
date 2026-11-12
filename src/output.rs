use crate::{Error, ErrorCode, Image, Result};
use base64::{Engine, engine::general_purpose::STANDARD};

/// Encode an opaque, deterministic RGB PNG with explicit physical resolution.
pub fn encode_png(image: &Image, dpi: f64) -> Result<Vec<u8>> {
    if !dpi.is_finite() || dpi <= 0.0 || dpi > 100_000.0 {
        return Err(Error::new(
            ErrorCode::InvalidArguments,
            "DPI must be finite and in (0, 100000]",
        ));
    }
    let mut result = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut result, image.width(), image.height());
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_pixel_dims(Some(png::PixelDimensions {
            xppu: (dpi / 0.0254).round().max(1.0) as u32,
            yppu: (dpi / 0.0254).round().max(1.0) as u32,
            unit: png::Unit::Meter,
        }));
        let mut writer = encoder.write_header().map_err(encoding_error)?;
        writer
            .write_image_data(image.rgb())
            .map_err(encoding_error)?;
        writer.finish().map_err(encoding_error)?;
    }
    Ok(result)
}

fn encoding_error(error: png::EncodingError) -> Error {
    Error::new(ErrorCode::Encoding, error.to_string())
}

/// Embed the complete composited picture, never an isolated destination-dependent overlay.
/// This is a raster-backed SVG, not editable vector geometry.
pub fn encode_svg(image: &Image, dpi: f64) -> Result<Vec<u8>> {
    let png = encode_png(image, dpi)?;
    let width = image.width();
    let height = image.height();
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{:.8}in\" height=\"{:.8}in\" viewBox=\"0 0 {width} {height}\"><image width=\"{width}\" height=\"{height}\" xlink:href=\"data:image/png;base64,{}\"/></svg>\n",
        width as f64 / dpi, height as f64 / dpi, STANDARD.encode(png),
    ).into_bytes())
}
