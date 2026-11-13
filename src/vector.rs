use crate::model::*;
use crate::raster::{State, copy_color, layout_text, line_points, output_size, text_policy};
use crate::{RenderOptions, output};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::fmt::Write;

#[derive(Clone, Copy, Debug)]
pub enum VectorFormat {
    Svg,
    Pdf,
}

#[derive(Debug)]
pub struct VectorOutput {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub diagnostics: Vec<Diagnostic>,
}

enum Segment {
    Move(f64, f64),
    Line(f64, f64),
    Curve([f64; 6]),
    Close,
}
#[derive(Default)]
struct Path(Vec<Segment>, bool);
impl Path {
    fn add(&mut self, segment: Segment, budget: &mut Budget<'_>) -> Result<()> {
        budget.allocate(std::mem::size_of::<Segment>())?;
        budget.work(1)?;
        self.0.push(segment);
        Ok(())
    }
    fn rect(&mut self, r: Rect, budget: &mut Budget<'_>) -> Result<()> {
        if r.width() == 0 || r.height() == 0 {
            return Ok(());
        }
        self.add(Segment::Move(r.left as f64, r.top as f64), budget)?;
        self.add(Segment::Line(r.right as f64, r.top as f64), budget)?;
        self.add(Segment::Line(r.right as f64, r.bottom as f64), budget)?;
        self.add(Segment::Line(r.left as f64, r.bottom as f64), budget)?;
        self.add(Segment::Close, budget)
    }
    fn region(&mut self, r: &Region, budget: &mut Budget<'_>) -> Result<()> {
        if r.mask.is_none() {
            return self.rect(r.bounds, budget);
        }
        budget.work((r.bounds.width() * r.bounds.height()) as u64)?;
        if r.bounds.width() == 0 {
            return Ok(());
        }
        for y in r.bounds.top..r.bounds.bottom {
            let mut x = r.bounds.left;
            while x < r.bounds.right {
                if !r.contains(x, y) {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < r.bounds.right && r.contains(x, y) {
                    x += 1;
                }
                self.rect(
                    Rect {
                        top: y,
                        bottom: y + 1,
                        left: start,
                        right: x,
                    },
                    budget,
                )?;
            }
        }
        Ok(())
    }
    fn ellipse_arc(
        &mut self,
        r: Rect,
        start: f64,
        sweep: f64,
        move_to: bool,
        budget: &mut Budget<'_>,
    ) -> Result<()> {
        let (rx, ry) = (r.width() as f64 / 2.0, r.height() as f64 / 2.0);
        let (cx, cy) = (r.left as f64 + rx, r.top as f64 + ry);
        let point = |t: f64| (cx + rx * t.sin(), cy - ry * t.cos());
        let pieces = (sweep.abs() / 90.0).ceil().max(1.0) as usize;
        let step = sweep.to_radians() / pieces as f64;
        let mut t = start.to_radians();
        let (x, y) = point(t);
        self.add(
            if move_to {
                Segment::Move(x, y)
            } else {
                Segment::Line(x, y)
            },
            budget,
        )?;
        for _ in 0..pieces {
            let end = t + step;
            let k = 4.0 / 3.0 * (step / 4.0).tan();
            let (x1, y1) = point(t);
            let (x2, y2) = point(end);
            self.add(
                Segment::Curve([
                    x1 + k * rx * t.cos(),
                    y1 + k * ry * t.sin(),
                    x2 - k * rx * end.cos(),
                    y2 - k * ry * end.sin(),
                    x2,
                    y2,
                ]),
                budget,
            )?;
            t = end;
        }
        Ok(())
    }
    fn outline(
        &mut self,
        shape: &Shape,
        r: Rect,
        state: &State<'_>,
        budget: &mut Budget<'_>,
    ) -> Result<()> {
        if r.width() == 0 || r.height() == 0 {
            return Ok(());
        }
        match shape {
            Shape::Rect(_) | Shape::Region(_) => return self.rect(r, budget),
            Shape::Oval(_) => self.ellipse_arc(r, 0.0, 360.0, true, budget)?,
            Shape::Arc { start, sweep, .. } => {
                if *sweep == 0 {
                    return Ok(());
                }
                let sweep = sweep.clamp(&-360, &360);
                if sweep.abs() < 360 {
                    self.add(
                        Segment::Move(
                            r.left as f64 + r.width() as f64 / 2.0,
                            r.top as f64 + r.height() as f64 / 2.0,
                        ),
                        budget,
                    )?;
                }
                self.ellipse_arc(
                    r,
                    start.rem_euclid(360) as f64,
                    *sweep as f64,
                    sweep.abs() == 360,
                    budget,
                )?;
            }
            Shape::RoundRect(_) => {
                let w = state.oval.x.min(r.width() as i32);
                let h = state.oval.y.min(r.height() as i32);
                if w == 0 || h == 0 {
                    return self.rect(r, budget);
                }
                for (corner, start, first) in [
                    (
                        Rect {
                            left: r.right - w,
                            bottom: r.top + h,
                            ..r
                        },
                        0.0,
                        true,
                    ),
                    (
                        Rect {
                            left: r.right - w,
                            top: r.bottom - h,
                            ..r
                        },
                        90.0,
                        false,
                    ),
                    (
                        Rect {
                            right: r.left + w,
                            top: r.bottom - h,
                            ..r
                        },
                        180.0,
                        false,
                    ),
                    (
                        Rect {
                            right: r.left + w,
                            bottom: r.top + h,
                            ..r
                        },
                        270.0,
                        false,
                    ),
                ] {
                    self.ellipse_arc(corner, start, 90.0, first, budget)?;
                }
            }
            Shape::Polygon { .. } => unreachable!(),
        }
        self.add(Segment::Close, budget)
    }
    fn text(&self, pdf: bool) -> String {
        let mut s = String::new();
        for segment in &self.0 {
            match segment {
                Segment::Move(x, y) | Segment::Line(x, y) => {
                    let is_move = matches!(segment, Segment::Move(..));
                    if pdf {
                        writeln!(s, "{x:.8} {y:.8} {}", if is_move { "m" } else { "l" }).unwrap();
                    } else {
                        write!(s, "{}{x:.8} {y:.8} ", if is_move { "M" } else { "L" }).unwrap();
                    }
                }
                Segment::Curve(v) => {
                    if !pdf {
                        s.push('C');
                    }
                    for n in v {
                        write!(s, "{n:.8} ").unwrap();
                    }
                    s.push_str(if pdf { "c\n" } else { "" });
                }
                Segment::Close => s.push_str(if pdf { "h\n" } else { "Z " }),
            }
        }
        s
    }
}
fn unsupported(message: &str) -> Error {
    Error::new(
        ErrorCode::Unsupported,
        format!("{message}; use explicit whole-picture --vector-policy raster if acceptable"),
    )
}
fn color(state: &State<'_>, verb: Verb) -> Result<Option<[u8; 3]>> {
    let (pattern, mode) = state.drawing(verb);
    match pattern {
        Pattern::Solid(rgb) => Ok(Some(*rgb)),
        Pattern::Bits(bits)
            if matches!(mode, 8 | 12)
                && (bits.iter().all(|&b| b == 255) || bits.iter().all(|&b| b == 0)) =>
        {
            let source = pattern.sample(0, 0);
            Ok(Some(copy_color(
                if mode == 12 {
                    source.map(|v| !v)
                } else {
                    source
                },
                state,
            )))
        }
        Pattern::Color { .. } => Ok(None),
        Pattern::Bits(_) if matches!(mode, 8 | 12 | 44) => Ok(None),
        _ => Err(unsupported(
            "vector paths require copy patterns; destination-dependent modes need the complete raster destination",
        )),
    }
}

/// Emit genuine paths. Never silently substitutes raster content for unsupported vector operations.
pub fn export_vector(
    picture: &Picture,
    options: &RenderOptions,
    dpi: f64,
    format: VectorFormat,
) -> Result<VectorOutput> {
    options.limits.validate()?;
    if picture.operations.len() > options.limits.max_operations {
        return Err(Error::new(
            ErrorCode::ResourceLimit,
            "operation limit exceeded",
        ));
    }
    let (width, height) = output_size(picture.frame, options)?;
    let (w, h) = output::physical_size(width as u32, height as u32, dpi)?;
    let pdf = matches!(format, VectorFormat::Pdf);
    let mut budget = Budget::new(&options.limits);
    let mut content = String::new();
    let mut resources = Resources::default();
    let mut state = State {
        highlight_color: options.highlight,
        ..Default::default()
    };
    let mut diagnostics = vec![
        Diagnostic::new(
            "unverified_fidelity",
            "Portable vector presentation, not classic-screen pixel equivalence; curves use cubic Bezier approximations.",
            None,
        ),
        Diagnostic::new(
            "vector_presentation",
            "Editable paths; bitmap fonts/regions are pixel-run geometry, not smooth outlines. No automatic raster substitution.",
            None,
        ),
    ];
    let (fw, fh) = (picture.frame.width(), picture.frame.height());
    if pdf {
        writeln!(
            content,
            "q\n{:.16} 0 0 {:.16} 0 {h:.8} cm\n0 0 {fw} {fh} re W n",
            w / fw as f64,
            -h / fh as f64
        )
        .unwrap();
    } else {
        write!(content, "<svg xmlns=\"http://www.w3.org/2000/svg\" overflow=\"hidden\" width=\"{:.8}in\" height=\"{:.8}in\" viewBox=\"0 0 {fw} {fh}\">", w / 72.0, h / 72.0).unwrap();
    }
    let mut background = Path::default();
    background.rect(
        Rect {
            top: 0,
            left: 0,
            bottom: fh as i32,
            right: fw as i32,
        },
        &mut budget,
    )?;
    emit(
        &mut content,
        &background,
        None,
        options.background,
        (0, 0),
        0,
        pdf,
        &mut budget,
    )?;
    for (index, op) in picture.operations.iter().enumerate() {
        let result = (|| {
            budget.work(1)?;
            if state.update(&op.command, options)? {
                return Ok(());
            }
            let mut path = Path::default();
            let rgb;
            let mut pattern_verb = Verb::Frame;
            match &op.command {
                Command::Draw(verb, shape) => {
                    pattern_verb = *verb;
                    path.1 = true;
                    rgb = color(&state, *verb)?;
                    if matches!((verb, shape), (Verb::Frame, Shape::Arc { .. })) {
                        return Err(unsupported("vector arc framing is not implemented"));
                    }
                    if !matches!(shape, Shape::Rect(_) | Shape::Region(_)) {
                        if options.fidelity == Fidelity::Strict {
                            return Err(unsupported(
                                "curves/polygons require --fidelity approximate",
                            ));
                        }
                        diagnostics.push(Diagnostic::new(
                            "approximation",
                            "Continuous vector geometry; not classic pixel edge rules.",
                            Some(op.offset),
                        ));
                    }
                    match shape {
                        Shape::Polygon { points, .. } => {
                            if matches!(verb, Verb::Frame) {
                                return Err(unsupported(
                                    "vector polygon framing is not implemented",
                                ));
                            }
                            for (i, p) in points.iter().enumerate() {
                                path.add(
                                    if i == 0 {
                                        Segment::Move(p.x as f64, p.y as f64)
                                    } else {
                                        Segment::Line(p.x as f64, p.y as f64)
                                    },
                                    &mut budget,
                                )?;
                            }
                            path.add(Segment::Close, &mut budget)?;
                        }
                        Shape::Region(r) if r.mask.is_some() => {
                            if matches!(verb, Verb::Frame) {
                                return Err(unsupported(
                                    "vector complex region framing is not implemented",
                                ));
                            }
                            path.region(r, &mut budget)?;
                        }
                        _ => {
                            let r = match shape {
                                Shape::Rect(r)
                                | Shape::Oval(r)
                                | Shape::RoundRect(r)
                                | Shape::Arc { bounds: r, .. } => *r,
                                Shape::Region(r) => r.bounds,
                                _ => unreachable!(),
                            };
                            if matches!(verb, Verb::Frame) && (state.pen.x == 0 || state.pen.y == 0)
                            {
                                state.highlight = false;
                                return Ok(());
                            }
                            path.outline(shape, r, &state, &mut budget)?;
                            if matches!(verb, Verb::Frame) {
                                let oval = state.oval;
                                state.oval = Point {
                                    x: (oval.x.min(r.width() as i32) - 2 * state.pen.x).max(0),
                                    y: (oval.y.min(r.height() as i32) - 2 * state.pen.y).max(0),
                                };
                                path.outline(
                                    shape,
                                    r.inset(state.pen.x, state.pen.y),
                                    &state,
                                    &mut budget,
                                )?;
                                state.oval = oval;
                            }
                        }
                    }
                }
                Command::Line(_, _) | Command::LineFrom(_) | Command::ShortLineFrom(_) => {
                    rgb = color(&state, Verb::Frame)?;
                    let (from, to) = line_points(&op.command, &state)?;
                    state.pen_location = to;
                    if from.x != to.x && from.y != to.y {
                        return Err(unsupported(
                            "vector diagonal pen coverage is not implemented",
                        ));
                    }
                    if state.pen.x != 0 && state.pen.y != 0 {
                        let r = Rect {
                            top: from.y.min(to.y),
                            left: from.x.min(to.x),
                            bottom: from.y.max(to.y),
                            right: from.x.max(to.x),
                        };
                        let end = Rect {
                            top: 0,
                            left: 0,
                            bottom: r.bottom,
                            right: r.right,
                        }
                        .translated(state.pen)?;
                        path.rect(
                            Rect {
                                bottom: end.bottom,
                                right: end.right,
                                ..r
                            },
                            &mut budget,
                        )?;
                    }
                }
                Command::Text { .. } => {
                    text_policy(&state, options, &mut diagnostics, op.offset)?;
                    let run = layout_text(&mut state, &op.command, options, &mut budget)?;
                    let mut colors = match state.text.base_mode() {
                        0 => [Some(state.foreground), Some(state.background)],
                        4 => [Some(state.background), Some(state.foreground)],
                        1 if state.foreground == [0; 3] => [Some([0; 3]), None],
                        3 if state.background == [255; 3] => [Some([255; 3]), None],
                        5 if state.foreground == [0; 3] => [None, Some([0; 3])],
                        7 if state.background == [255; 3] => [None, Some([255; 3])],
                        36 => [
                            if state.foreground == state.background {
                                None
                            } else {
                                Some(state.foreground)
                            },
                            None,
                        ],
                        _ => {
                            return Err(unsupported(
                                "this NFNT text mode requires destination pixels; use explicit whole-picture raster",
                            ));
                        }
                    };
                    if state.text.masked() {
                        colors[1] = None;
                    }
                    let viewport = picture.frame.translated(state.origin)?;
                    let mask = run.mask(viewport, state.text.masked(), &mut budget)?;
                    for (plane, color) in colors.into_iter().enumerate() {
                        let Some(color) = color else { continue };
                        let mut text_path = Path::default();
                        budget.work((mask.bounds.width() * mask.bounds.height()) as u64)?;
                        for y in mask.bounds.top..mask.bounds.bottom {
                            let covered = |x| {
                                mask.ink(x, y) == (plane == 0)
                                    && state.clip.is_none_or(|clip| clip.contains(x, y))
                            };
                            let mut x = mask.bounds.left;
                            while x < mask.bounds.right {
                                if !covered(x) {
                                    x += 1;
                                    continue;
                                }
                                let start = x;
                                while x < mask.bounds.right && covered(x) {
                                    x += 1;
                                }
                                text_path.rect(
                                    Rect {
                                        top: y,
                                        bottom: y + 1,
                                        left: start,
                                        right: x,
                                    },
                                    &mut budget,
                                )?;
                            }
                        }
                        emit(
                            &mut content,
                            &text_path,
                            None,
                            color,
                            (-(viewport.left as i64), -(viewport.top as i64)),
                            index + 1,
                            pdf,
                            &mut budget,
                        )?;
                    }
                    diagnostics.push(Diagnostic::new("explicit_bitmap_font", format!("NFNT id={}, size={}, face={}; glyph pixel runs, no replacement or outline substitution", state.text.id, state.text.size, state.text.face), Some(op.offset)));
                    state.highlight = false;
                    return Ok(());
                }
                Command::Bitmap(bitmap) => {
                    if !matches!(bitmap.mode, 0 | 4 | 36) {
                        return Err(unsupported(
                            "preserved bitmap placement requires srcCopy, notSrcCopy or transparent",
                        ));
                    }
                    if bitmap.mode == 4 && matches!(bitmap.pixels, Pixels::Rgb(_)) {
                        if options.fidelity == Fidelity::Strict {
                            return Err(unsupported(
                                "color bitmap notSrcCopy requires --fidelity approximate",
                            ));
                        }
                        diagnostics.push(Diagnostic::new(
                            "approximation",
                            "Color bitmap inversion uses the portable RGB profile.",
                            Some(op.offset),
                        ));
                    }
                    emit_bitmap(
                        &mut content,
                        &mut resources,
                        bitmap,
                        &state,
                        picture.frame,
                        index + 1,
                        pdf,
                        &mut budget,
                    )?;
                    diagnostics.push(Diagnostic::new("embedded_bitmap","Copy-only bitmap placed with nearest-neighbor sampling and explicit masks; other geometry stays vector.",Some(op.offset)));
                    state.highlight = false;
                    return Ok(());
                }
                Command::Unsupported { feature, .. } => return Err(unsupported(feature)),
                Command::Comment { .. } => {
                    if options.fidelity == Fidelity::Strict {
                        return Err(unsupported(
                            "printer comments require --fidelity approximate",
                        ));
                    }
                    diagnostics.push(Diagnostic::new(
                        "approximation",
                        "Printer comment semantics ignored.",
                        Some(op.offset),
                    ));
                    return Ok(());
                }
                Command::Reserved(_) => {
                    diagnostics.push(Diagnostic::new(
                        "reserved_opcode",
                        "Reserved opcode retained without drawing semantics.",
                        Some(op.offset),
                    ));
                    return Ok(());
                }
                _ => unreachable!("state command"),
            }
            let mut clip = Path::default();
            if let Some(region) = state.clip {
                clip.region(region, &mut budget)?;
            }
            let viewport = picture.frame.translated(state.origin)?;
            // Negative translation is represented in i64 to cover i32::MIN coordinates.
            let delta = (-(viewport.left as i64), -(viewport.top as i64));
            if let Some(rgb) = rgb {
                emit(
                    &mut content,
                    &path,
                    state.clip.map(|_| &clip),
                    rgb,
                    delta,
                    index + 1,
                    pdf,
                    &mut budget,
                )?;
            } else {
                emit_pattern(
                    &mut content,
                    &mut resources,
                    &path,
                    state.clip.map(|_| &clip),
                    state.drawing(pattern_verb).0,
                    state.drawing(pattern_verb).1,
                    &state,
                    (w / fw as f64, h / fh as f64, h),
                    picture.frame,
                    index + 1,
                    pdf,
                    &mut budget,
                )?;
            }
            state.highlight = false;
            Ok(())
        })();
        result.map_err(|e: Error| e.at(op.offset).opcode(op.opcode))?;
    }
    if pdf {
        content.push_str("Q\n");
    } else {
        content.push_str("</svg>\n");
    }
    let bytes = if pdf {
        let dictionary = resources.dictionary();
        output::pdf_objects(w, h, content.into_bytes(), &dictionary, resources.objects)?
    } else {
        content.into_bytes()
    };
    Ok(VectorOutput {
        bytes,
        width: width as u32,
        height: height as u32,
        diagnostics,
    })
}

#[allow(clippy::too_many_arguments)]
fn emit(
    out: &mut String,
    path: &Path,
    clip: Option<&Path>,
    color: [u8; 3],
    delta: (i64, i64),
    id: usize,
    pdf: bool,
    budget: &mut Budget<'_>,
) -> Result<()> {
    // Serialization can be much larger than the compact path; charge before allocating it.
    let segments = path.0.len() + clip.map_or(0, |p| p.0.len());
    budget.allocate(
        segments
            .checked_mul(256)
            .and_then(|n| n.checked_add(1024))
            .ok_or_else(|| {
                Error::new(ErrorCode::ResourceLimit, "vector byte accounting overflow")
            })?,
    )?;
    let [r, g, b] = color;
    let (x, y) = delta;
    if pdf {
        writeln!(
            out,
            "q\n1 0 0 1 {x} {y} cm\n{:.8} {:.8} {:.8} rg",
            r as f64 / 255.0,
            g as f64 / 255.0,
            b as f64 / 255.0
        )
        .unwrap();
        if let Some(clip) = clip {
            writeln!(out, "{}W* n", clip.text(true)).unwrap();
        }
        writeln!(
            out,
            "{}{}\nQ",
            path.text(true),
            if path.1 { "f*" } else { "f" }
        )
        .unwrap();
    } else {
        write!(out, "<g transform=\"translate({x} {y})\">").unwrap();
        if let Some(clip) = clip {
            write!(out, "<defs><clipPath id=\"c{id}\" clipPathUnits=\"userSpaceOnUse\"><path clip-rule=\"evenodd\" d=\"{}\"/></clipPath></defs><g clip-path=\"url(#c{id})\">", clip.text(false)).unwrap();
        }
        write!(
            out,
            "<path fill=\"#{r:02x}{g:02x}{b:02x}\" fill-rule=\"{}\" d=\"{}\"/>",
            if path.1 { "evenodd" } else { "nonzero" },
            path.text(false)
        )
        .unwrap();
        if clip.is_some() {
            out.push_str("</g>");
        }
        out.push_str("</g>");
    }
    Ok(())
}

struct PixelsImage {
    width: usize,
    height: usize,
    rgb: Vec<u8>,
    alpha: Option<Vec<u8>>,
}
#[derive(Default)]
struct Resources {
    objects: Vec<Vec<u8>>,
    images: Vec<usize>,
    patterns: Vec<usize>,
}
impl Resources {
    fn object(&mut self, data: Vec<u8>) -> usize {
        let id = self.objects.len() + 5;
        self.objects.push(data);
        id
    }
    fn image(&mut self, image: &PixelsImage, budget: &mut Budget<'_>) -> Result<usize> {
        let mask = if let Some(alpha) = &image.alpha {
            budget.allocate(alpha.len() + 256)?;
            Some(self.object(output::pdf_stream(&format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Interpolate false",image.width,image.height),alpha)))
        } else {
            None
        };
        budget.allocate(image.rgb.len() + 512)?;
        let mask = mask.map_or(String::new(), |id| format!("/SMask {id} 0 R"));
        let id=self.object(output::pdf_stream(&format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Interpolate false {mask}",image.width,image.height),&image.rgb));
        self.images.push(id);
        Ok(id)
    }
    fn dictionary(&self) -> String {
        let mut s = String::new();
        if !self.images.is_empty() {
            s.push_str("/XObject << ");
            for id in &self.images {
                write!(s, "/Im{id} {id} 0 R ").unwrap();
            }
            s.push_str(">> ");
        }
        if !self.patterns.is_empty() {
            s.push_str("/Pattern << ");
            for id in &self.patterns {
                write!(s, "/P{id} {id} 0 R ").unwrap();
            }
            s.push_str(">> ");
        }
        s
    }
}
fn image_uri(image: &PixelsImage, budget: &mut Budget<'_>) -> Result<String> {
    budget.allocate(
        image
            .width
            .checked_mul(image.height)
            .and_then(|n| n.checked_mul(20))
            .and_then(|n| n.checked_add(8192))
            .ok_or_else(|| Error::new(ErrorCode::ResourceLimit, "embedded PNG size overflow"))?,
    )?;
    let rgba = image.alpha.as_ref().map(|alpha| {
        let mut rgba = Vec::with_capacity(alpha.len() * 4);
        for (pixel, &a) in image.rgb.chunks_exact(3).zip(alpha) {
            rgba.extend_from_slice(pixel);
            rgba.push(a);
        }
        rgba
    });
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, image.width as u32, image.height as u32);
        encoder.set_color(if rgba.is_some() {
            png::ColorType::Rgba
        } else {
            png::ColorType::Rgb
        });
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| Error::new(ErrorCode::Encoding, e.to_string()))?;
        writer
            .write_image_data(rgba.as_deref().unwrap_or(&image.rgb))
            .map_err(|e| Error::new(ErrorCode::Encoding, e.to_string()))?;
        writer
            .finish()
            .map_err(|e| Error::new(ErrorCode::Encoding, e.to_string()))?;
    }
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}
#[allow(clippy::too_many_arguments)]
fn emit_bitmap(
    out: &mut String,
    resources: &mut Resources,
    bitmap: &Bitmap,
    state: &State<'_>,
    frame: Rect,
    id: usize,
    pdf: bool,
    budget: &mut Budget<'_>,
) -> Result<()> {
    let viewport = frame.translated(state.origin)?;
    let mut area = bitmap.destination.intersect(viewport);
    if let Some(clip) = state.clip {
        area = area.intersect(clip.bounds);
    }
    if area.width() == 0 || area.height() == 0 {
        return Ok(());
    }
    let count = budget.limits.pixels(area.width(), area.height())?;
    budget.allocate(count * 4)?;
    budget.work(count as u64)?;
    let mut image = PixelsImage {
        width: area.width(),
        height: area.height(),
        rgb: vec![0; count * 3],
        alpha: None,
    };
    let mut alpha = vec![0; count];
    for y in area.top..area.bottom {
        for x in area.left..area.right {
            if state.clip.is_some_and(|c| !c.contains(x, y)) {
                continue;
            }
            if let Some(source) = bitmap.sample(x, y) {
                let i = (y - area.top) as usize * area.width() + (x - area.left) as usize;
                if bitmap.mode == 36 && source == state.background {
                    continue;
                }
                let rgb = if bitmap.raw_copy || bitmap.mode == 36 {
                    source
                } else {
                    copy_color(
                        if bitmap.mode == 4 {
                            source.map(|v| !v)
                        } else {
                            source
                        },
                        state,
                    )
                };
                image.rgb[i * 3..i * 3 + 3].copy_from_slice(&rgb);
                alpha[i] = 255;
            }
        }
    }
    if alpha.iter().all(|&a| a == 0) {
        return Ok(());
    }
    if alpha.iter().any(|&a| a != 255) {
        image.alpha = Some(alpha);
    }
    let x = area.left as i64 - viewport.left as i64;
    let y = area.top as i64 - viewport.top as i64;
    budget.allocate(1024)?;
    if pdf {
        let object = resources.image(&image, budget)?;
        writeln!(
            out,
            "q\n{} 0 0 -{} {x} {} cm\n/Im{object} Do\nQ",
            image.width,
            image.height,
            y + image.height as i64
        )
        .unwrap();
    } else {
        let uri = image_uri(&image, budget)?;
        write!(out,"<image xmlns:xlink=\"http://www.w3.org/1999/xlink\" id=\"image{id}\" x=\"{x}\" y=\"{y}\" width=\"{}\" height=\"{}\" image-rendering=\"pixelated\" xlink:href=\"{uri}\"/>",image.width,image.height).unwrap();
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn emit_pattern(
    out: &mut String,
    resources: &mut Resources,
    path: &Path,
    clip: Option<&Path>,
    pattern: &Pattern,
    mode: u16,
    state: &State<'_>,
    scale: (f64, f64, f64),
    frame: Rect,
    id: usize,
    pdf: bool,
    budget: &mut Budget<'_>,
) -> Result<()> {
    let (width, height) = match pattern {
        Pattern::Color { bounds, .. } => (bounds.width(), bounds.height()),
        Pattern::Bits(_) => (8, 8),
        Pattern::Solid(_) => unreachable!("uniform color"),
    };
    let n = budget.limits.pixels(width, height)?;
    budget.allocate(n * 4)?;
    budget.work(n as u64)?;
    let mut image = PixelsImage {
        width,
        height,
        rgb: Vec::with_capacity(n * 3),
        alpha: None,
    };
    let mut alpha = Vec::with_capacity(n);
    for y in 0..height {
        for x in 0..width {
            let color = pattern.sample(x as i32, y as i32);
            let bit_pattern = matches!(pattern, Pattern::Bits(_));
            alpha.push(if bit_pattern && mode == 44 && color == state.background {
                0
            } else {
                255
            });
            image.rgb.extend_from_slice(&if bit_pattern && mode != 44 {
                copy_color(if mode == 12 { color.map(|v| !v) } else { color }, state)
            } else {
                color
            });
        }
    }
    if alpha.contains(&0) {
        image.alpha = Some(alpha);
    }
    let viewport = frame.translated(state.origin)?;
    let (x, y) = (-(viewport.left as i64), -(viewport.top as i64));
    let segments = path.0.len() + clip.map_or(0, |c| c.0.len());
    budget.allocate(
        segments
            .checked_mul(256)
            .and_then(|n| n.checked_add(2048))
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::ResourceLimit,
                    "pattern serialization size overflow",
                )
            })?,
    )?;
    if pdf {
        let image_id = resources.image(&image, budget)?;
        let (sx, sy, h) = scale;
        let matrix = format!(
            "{sx:.16} 0 0 {:.16} {:.16} {:.16}",
            -sy,
            -sx * frame.left as f64,
            h + sy * frame.top as f64
        );
        let data = format!("q\n{width} 0 0 -{height} 0 {height} cm\n/Im{image_id} Do\nQ\n");
        let object=resources.object(output::pdf_stream(&format!("/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 {width} {height}] /XStep {width} /YStep {height} /Matrix [{matrix}] /Resources << /XObject << /Im{image_id} {image_id} 0 R >> >>"),data.as_bytes()));
        resources.patterns.push(object);
        writeln!(out, "q\n1 0 0 1 {x} {y} cm").unwrap();
        if let Some(clip) = clip {
            writeln!(out, "{}W* n", clip.text(true)).unwrap();
        }
        writeln!(
            out,
            "/Pattern cs /P{object} scn\n{}{}\nQ",
            path.text(true),
            if path.1 { "f*" } else { "f" }
        )
        .unwrap();
    } else {
        let uri = image_uri(&image, budget)?;
        write!(out,"<g transform=\"translate({x} {y})\"><defs><pattern id=\"p{id}\" patternUnits=\"userSpaceOnUse\" width=\"{width}\" height=\"{height}\" patternTransform=\"translate({} {})\"><image xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{width}\" height=\"{height}\" image-rendering=\"pixelated\" xlink:href=\"{uri}\"/></pattern></defs>",state.origin.x,state.origin.y).unwrap();
        if let Some(clip) = clip {
            write!(out,"<defs><clipPath id=\"c{id}\" clipPathUnits=\"userSpaceOnUse\"><path clip-rule=\"evenodd\" d=\"{}\"/></clipPath></defs><g clip-path=\"url(#c{id})\">",clip.text(false)).unwrap();
        }
        write!(
            out,
            "<path fill=\"url(#p{id})\" fill-rule=\"{}\" d=\"{}\"/>",
            if path.1 { "evenodd" } else { "nonzero" },
            path.text(false)
        )
        .unwrap();
        if clip.is_some() {
            out.push_str("</g>");
        }
        out.push_str("</g>");
    }
    Ok(())
}
