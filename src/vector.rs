use crate::model::*;
use crate::raster::{State, layout_text, line_points, output_size};
use crate::{RenderOptions, output};
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
fn color(state: &State<'_>, verb: Verb) -> Result<[u8; 3]> {
    let (pattern, mode) = state.drawing(verb);
    match pattern {
        Pattern::Solid(rgb) => Ok(*rgb),
        Pattern::Bits(bits) if mode == 8 && bits.iter().all(|&b| b == 255) => Ok(state.foreground),
        Pattern::Bits(bits) if mode == 8 && bits.iter().all(|&b| b == 0) => Ok(state.background),
        _ => Err(unsupported(
            "vector paths require uniform copy patterns; destination-dependent modes and tiled patterns are not representable by this backend",
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
            match &op.command {
                Command::Draw(verb, shape) => {
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
                    let glyphs = layout_text(&mut state, &op.command, options, &mut budget)?;
                    rgb = match state.text.mode {
                        1 if state.foreground == [0; 3] => [0; 3],
                        3 if state.background == [255; 3] => [255; 3],
                        _ => {
                            return Err(unsupported(
                                "colored NFNT Boolean text requires destination pixels",
                            ));
                        }
                    };
                    for placed in glyphs {
                        if placed.glyph.width == 0 {
                            continue;
                        }
                        for y in 0..placed.glyph.height {
                            let mut x = 0;
                            while x < placed.glyph.width {
                                if !placed.glyph.ink(x, y) {
                                    x += 1;
                                    continue;
                                }
                                let start = x;
                                while x < placed.glyph.width && placed.glyph.ink(x, y) {
                                    x += 1;
                                }
                                path.rect(
                                    Rect {
                                        top: placed.bounds.top + y as i32,
                                        bottom: placed.bounds.top + y as i32 + 1,
                                        left: placed.bounds.left + start as i32,
                                        right: placed.bounds.left + x as i32,
                                    },
                                    &mut budget,
                                )?;
                            }
                        }
                    }
                    diagnostics.push(Diagnostic::new("explicit_bitmap_font", format!("NFNT id={}, size={}, face={}; glyph pixel runs, no replacement or outline substitution", state.text.id, state.text.size, state.text.face), Some(op.offset)));
                }
                Command::Bitmap(_) => {
                    return Err(unsupported(
                        "bitmap instructions in preserved vector output are not yet implemented",
                    ));
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
        output::pdf_document(w, h, content.into_bytes(), None)?
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
