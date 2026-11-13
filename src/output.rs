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
    physical_size(image.width(), image.height(), dpi)?;
    let png = encode_png(image, dpi)?;
    let width = image.width();
    let height = image.height();
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{:.8}in\" height=\"{:.8}in\" viewBox=\"0 0 {width} {height}\"><image width=\"{width}\" height=\"{height}\" xlink:href=\"data:image/png;base64,{}\"/></svg>\n",
        width as f64 / dpi, height as f64 / dpi, STANDARD.encode(png),
    ).into_bytes())
}

pub(crate) fn physical_size(width: u32, height: u32, dpi: f64) -> Result<(f64, f64)> {
    let w = width as f64 * 72.0 / dpi;
    let h = height as f64 * 72.0 / dpi;
    if dpi <= 0.0 || dpi > 100_000.0 || !dpi.is_finite() || !w.is_finite() || !h.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArguments,
            "invalid physical output size or DPI",
        ));
    }
    Ok((w, h))
}

pub(crate) fn pdf_stream(dictionary: &str, data: &[u8]) -> Vec<u8> {
    let mut result = format!("<< {dictionary} /Length {} >>\nstream\n", data.len()).into_bytes();
    result.extend_from_slice(data);
    result.extend_from_slice(b"\nendstream");
    result
}

pub(crate) fn pdf_document(
    width: f64,
    height: f64,
    content: Vec<u8>,
    image: Option<&Image>,
) -> Result<Vec<u8>> {
    let resources = if image.is_some() {
        "/XObject << /Im0 5 0 R >>"
    } else {
        ""
    };
    let mut extra = Vec::new();
    if let Some(image) = image {
        extra.push(pdf_stream(&format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8",image.width(),image.height()),image.rgb()));
    }
    pdf_objects(width, height, content, resources, extra)
}

pub(crate) fn pdf_objects(
    width: f64,
    height: f64,
    content: Vec<u8>,
    resources: &str,
    extra: Vec<Vec<u8>>,
) -> Result<Vec<u8>> {
    if width > 14400.0 || height > 14400.0 {
        return Err(Error::new(
            ErrorCode::InvalidArguments,
            "PDF 1.4 page exceeds 14400 points; increase DPI or reduce dimensions",
        ));
    }
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width:.8} {height:.8}] /Resources << {resources} >> /Contents 4 0 R >>").into_bytes(),
        pdf_stream("", &content),
    ];
    objects.extend(extra);
    let mut bytes = b"%PDF-1.4\n%\xd3\xe4\xf5\xf6\n".to_vec();
    let mut offsets = vec![0];
    for (i, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        bytes.extend_from_slice(object);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let start = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in &offsets[1..] {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    Ok(bytes)
}

/// PDF containing the entire composited RGB picture, not isolated overlays.
pub fn encode_pdf(image: &Image, dpi: f64) -> Result<Vec<u8>> {
    let (w, h) = physical_size(image.width(), image.height(), dpi)?;
    pdf_document(
        w,
        h,
        format!("q\n{w:.8} 0 0 {h:.8} 0 0 cm\n/Im0 Do\nQ\n").into_bytes(),
        Some(image),
    )
}
