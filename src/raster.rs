use crate::model::*;

#[derive(Clone, Debug)]
pub struct RenderOptions {
    pub fidelity: Fidelity,
    pub background: [u8; 3],
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub limits: Limits,
}
impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            fidelity: Fidelity::Strict,
            background: [255; 3],
            width: None,
            height: None,
            limits: Limits::default(),
        }
    }
}

#[derive(Debug)]
pub struct Image {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
}
impl Image {
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn rgb(&self) -> &[u8] {
        &self.rgb
    }
}
#[derive(Debug)]
pub struct Rendered {
    pub image: Image,
    pub diagnostics: Vec<Diagnostic>,
}

struct State<'a> {
    clip: Option<&'a Region>,
    foreground: [u8; 3],
    background: [u8; 3],
    pen_pattern: [u8; 8],
    background_pattern: [u8; 8],
    fill_pattern: [u8; 8],
    pen: Point,
    mode: u16,
}
impl Default for State<'_> {
    fn default() -> Self {
        Self {
            clip: None,
            foreground: [0; 3],
            background: [255; 3],
            pen_pattern: [255; 8],
            background_pattern: [0; 8],
            fill_pattern: [255; 8],
            pen: Point { x: 1, y: 1 },
            mode: 8,
        }
    }
}

struct Canvas {
    frame: Rect,
    rgb: Vec<u8>,
}
impl Canvas {
    fn apply(&mut self, x: i32, y: i32, source: [u8; 3], mode: u16, state: &State<'_>) {
        if !self.frame.contains(x, y) || state.clip.is_some_and(|clip| !clip.contains(x, y)) {
            return;
        }
        let index = ((y - self.frame.top) as usize * self.frame.width()
            + (x - self.frame.left) as usize)
            * 3;
        for (dest, src) in self.rgb[index..index + 3].iter_mut().zip(source) {
            *dest = transfer(src, *dest, mode);
        }
    }
    fn patterned(&mut self, x: i32, y: i32, verb: Verb, state: &State<'_>) {
        let (pattern, mode) = match verb {
            Verb::Frame | Verb::Paint => (&state.pen_pattern, state.mode),
            Verb::Erase => (&state.background_pattern, 8),
            Verb::Fill => (&state.fill_pattern, 8),
            Verb::Invert => {
                self.apply(x, y, [0; 3], 2, state);
                return;
            }
        };
        let bit = pattern[y.rem_euclid(8) as usize] & (0x80 >> x.rem_euclid(8));
        let color = if bit != 0 {
            state.foreground
        } else {
            state.background
        };
        self.apply(x, y, color, mode, state);
    }
}

// QuickDraw monochrome logical ink uses one for black; RGB uses zero for black.
fn transfer(source: u8, dest: u8, mode: u16) -> u8 {
    match mode & 7 {
        0 => source,
        1 => source & dest,
        2 => !(source ^ dest),
        3 => !source | dest,
        4 => !source,
        5 => !source & dest,
        6 => source ^ dest,
        _ => source | dest,
    }
}

fn require_approximate(
    options: &RenderOptions,
    diagnostics: &mut Vec<Diagnostic>,
    offset: usize,
    feature: &str,
) -> Result<()> {
    if options.fidelity == Fidelity::Strict {
        return Err(Error::new(
            ErrorCode::Unsupported,
            format!("{feature} requires --fidelity approximate"),
        )
        .at(offset));
    }
    diagnostics.push(Diagnostic::new("approximation", feature, Some(offset)));
    Ok(())
}

fn check_mode(mode: u16, pattern: bool) -> Result<()> {
    if (pattern && (8..=15).contains(&mode)) || (!pattern && mode <= 7) {
        Ok(())
    } else {
        Err(Error::new(
            ErrorCode::Unsupported,
            format!("unsupported transfer mode {mode}"),
        ))
    }
}

fn output_size(frame: Rect, options: &RenderOptions) -> Result<(usize, usize)> {
    let (w, h) = (frame.width() as u64, frame.height() as u64);
    let (width, height) = match (options.width, options.height) {
        (None, None) => (w, h),
        (Some(x), None) => (x as u64, (x as u64 * h + w / 2) / w),
        (None, Some(y)) => ((y as u64 * w + h / 2) / h, y as u64),
        (Some(x), Some(y)) => (x as u64, y as u64),
    };
    if width == 0 || height == 0 || width > u32::MAX as u64 || height > u32::MAX as u64 {
        return Err(Error::new(
            ErrorCode::InvalidArguments,
            "output dimensions are zero or out of range",
        ));
    }
    options.limits.pixels(width as usize, height as usize)?;
    Ok((width as usize, height as usize))
}

/// Render in order into an opaque RGB framebuffer, then optionally resample.
/// Destination-dependent operations always see the complete preceding framebuffer.
pub fn render(picture: &Picture, options: &RenderOptions) -> Result<Rendered> {
    options.limits.validate()?;
    if picture.operations.len() > options.limits.max_operations {
        return Err(Error::new(
            ErrorCode::ResourceLimit,
            "operation limit exceeded",
        ));
    }
    let frame = picture.frame;
    let (out_width, out_height) = output_size(frame, options)?;
    let pixels = options.limits.pixels(frame.width(), frame.height())?;
    let mut budget = Budget::new(&options.limits);
    budget.allocate(pixels * 3)?;
    budget.work(pixels as u64)?;
    let mut canvas = Canvas {
        frame,
        rgb: Vec::with_capacity(pixels * 3),
    };
    for _ in 0..pixels {
        canvas.rgb.extend_from_slice(&options.background);
    }
    let mut state = State::default();
    let mut diagnostics = vec![Diagnostic::new(
        "unverified_fidelity",
        "Portable RGB raster profile; no classic Mac rendering oracle or historical fonts were used to validate pixel identity.",
        None,
    )];
    for op in &picture.operations {
        let result = (|| {
            budget.work(1)?;
            match &op.command {
                Command::Nop | Command::Metadata(_) => {}
                Command::Clip(region) => state.clip = Some(region),
                Command::BackgroundPattern(p) => state.background_pattern = *p,
                Command::PenPattern(p) => state.pen_pattern = *p,
                Command::FillPattern(p) => state.fill_pattern = *p,
                Command::Foreground(c) => state.foreground = *c,
                Command::Background(c) => state.background = *c,
                Command::PenSize(p) => state.pen = *p,
                Command::PenMode(mode) => {
                    check_mode(*mode, true)?;
                    state.mode = *mode;
                }
                Command::Unsupported { feature, .. } => {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        format!("{feature} is not implemented"),
                    ));
                }
                Command::Comment { kind, .. } => {
                    require_approximate(
                        options,
                        &mut diagnostics,
                        op.offset,
                        &format!("printer/comment semantics ignored (kind {kind})"),
                    )?;
                }
                Command::Reserved(_) => {
                    diagnostics.push(Diagnostic::new(
                        "reserved_opcode",
                        "Reserved opcode retained but has no rendering semantics.",
                        Some(op.offset),
                    ));
                }
                Command::Line(from, to) => {
                    if from.x != to.x && from.y != to.y {
                        require_approximate(
                            options,
                            &mut diagnostics,
                            op.offset,
                            "diagonal line uses integer Bresenham coverage",
                        )?;
                    }
                    line(&mut canvas, *from, *to, &state, &mut budget)?;
                }
                Command::Draw(verb, shape) => {
                    if matches!(shape, Shape::Oval(_) | Shape::Polygon { .. })
                        || matches!(
                            (verb, shape),
                            (Verb::Frame, Shape::Region(Region { mask: Some(_), .. }))
                        )
                    {
                        require_approximate(
                            options,
                            &mut diagnostics,
                            op.offset,
                            "curve/polygon/complex region outline uses portable coverage",
                        )?;
                    }
                    draw(&mut canvas, *verb, shape, &state, &mut budget)?;
                }
                Command::Bitmap(bitmap) => {
                    check_mode(bitmap.mode, false)?;
                    if matches!(bitmap.pixels, Pixels::Rgb(_)) && bitmap.mode != 0 {
                        require_approximate(
                            options,
                            &mut diagnostics,
                            op.offset,
                            "color bitmap Boolean mode uses RGB ink-space logic",
                        )?;
                    }
                    blit(&mut canvas, bitmap, &state, &mut budget)?;
                }
            }
            Ok(())
        })();
        result.map_err(|mut e: Error| {
            if e.offset.is_none() {
                e.offset = Some(op.offset);
            }
            e.opcode(op.opcode)
        })?;
    }
    let rgb = if (out_width, out_height) == (frame.width(), frame.height()) {
        canvas.rgb
    } else {
        let count = out_width * out_height;
        budget.allocate(count * 3)?;
        budget.work(count as u64)?;
        let mut scaled = Vec::with_capacity(count * 3);
        for y in 0..out_height {
            let sy = y * frame.height() / out_height;
            for x in 0..out_width {
                let sx = x * frame.width() / out_width;
                let index = (sy * frame.width() + sx) * 3;
                scaled.extend_from_slice(&canvas.rgb[index..index + 3]);
            }
        }
        diagnostics.push(Diagnostic::new(
            "resampled",
            "Nearest-neighbor scaling applied after compositing.",
            None,
        ));
        scaled
    };
    Ok(Rendered {
        image: Image {
            width: out_width as u32,
            height: out_height as u32,
            rgb,
        },
        diagnostics,
    })
}

fn oval_contains(rect: Rect, x: i32, y: i32) -> bool {
    if !rect.contains(x, y) {
        return false;
    }
    let w = rect.width() as i128;
    let h = rect.height() as i128;
    let dx = 2 * (x - rect.left) as i128 + 1 - w;
    let dy = 2 * (y - rect.top) as i128 + 1 - h;
    dx * dx * h * h + dy * dy * w * w <= w * w * h * h
}

fn polygon_contains(points: &[Point], x: i32, y: i32) -> bool {
    let mut inside = false;
    let (px, py) = (2 * x as i64 + 1, 2 * y as i64 + 1);
    for (a, b) in points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
    {
        let (ax, ay, bx, by) = (
            2 * a.x as i64,
            2 * a.y as i64,
            2 * b.x as i64,
            2 * b.y as i64,
        );
        if (ay > py) != (by > py) {
            let lhs = (px - ax) * (by - ay);
            let rhs = (bx - ax) * (py - ay);
            if if by > ay { lhs < rhs } else { lhs > rhs } {
                inside = !inside;
            }
        }
    }
    inside
}

fn draw(
    canvas: &mut Canvas,
    verb: Verb,
    shape: &Shape,
    state: &State<'_>,
    budget: &mut Budget<'_>,
) -> Result<()> {
    if matches!(verb, Verb::Frame) && (state.pen.x == 0 || state.pen.y == 0) {
        return Ok(());
    }
    if let (Verb::Frame, Shape::Polygon { points, .. }) = (verb, shape) {
        let mut mask = coverage_mask(canvas, budget)?;
        for (a, b) in points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .take(points.len())
        {
            stamp_line(&mut mask, canvas.frame, *a, *b, state.pen, budget)?;
        }
        apply_mask(canvas, &mask, state, budget)?;
        return Ok(());
    }
    let bounds = match shape {
        Shape::Rect(r) | Shape::Oval(r) => *r,
        Shape::Polygon { bounds, .. } => *bounds,
        Shape::Region(region) => region.bounds,
    };
    let area = bounds.intersect(canvas.frame);
    if area.width() == 0 || area.height() == 0 {
        return Ok(());
    }
    let samples = (area.width() * area.height()) as u64;
    budget.work(samples)?;
    if let Shape::Polygon { points, .. } = shape {
        budget.work(
            samples
                .checked_mul(points.len() as u64)
                .ok_or_else(|| Error::new(ErrorCode::ResourceLimit, "polygon work overflow"))?,
        )?;
    }
    for y in area.top..area.bottom {
        for x in area.left..area.right {
            let inside = match shape {
                Shape::Rect(_) => true,
                Shape::Oval(rect) => oval_contains(*rect, x, y),
                Shape::Polygon { points, .. } => polygon_contains(points, x, y),
                Shape::Region(region) => region.contains(x, y),
            };
            if !inside {
                continue;
            }
            if matches!(verb, Verb::Frame) {
                let interior = match shape {
                    Shape::Rect(rect) => rect.inset(state.pen.x, state.pen.y).contains(x, y),
                    Shape::Oval(rect) => oval_contains(rect.inset(state.pen.x, state.pen.y), x, y),
                    Shape::Region(region) if region.mask.is_none() => {
                        region.bounds.inset(state.pen.x, state.pen.y).contains(x, y)
                    }
                    Shape::Region(region) => {
                        let mut interior = true;
                        'kernel: for dy in -state.pen.y..=state.pen.y {
                            for dx in -state.pen.x..=state.pen.x {
                                budget.work(1)?;
                                if !region.contains(x + dx, y + dy) {
                                    interior = false;
                                    break 'kernel;
                                }
                            }
                        }
                        interior
                    }
                    Shape::Polygon { .. } => unreachable!(),
                };
                if interior {
                    continue;
                }
            }
            canvas.patterned(x, y, verb, state);
        }
    }
    Ok(())
}

fn coverage_mask(canvas: &Canvas, budget: &mut Budget<'_>) -> Result<Vec<u8>> {
    let count = canvas.frame.width() * canvas.frame.height();
    // Cumulative allocation budget also bounds repeated full-frame temporary masks.
    budget.allocate(count)?;
    budget.work(count as u64)?;
    Ok(vec![0; count])
}
fn stamp_line(
    mask: &mut [u8],
    frame: Rect,
    from: Point,
    to: Point,
    pen: Point,
    budget: &mut Budget<'_>,
) -> Result<()> {
    if pen.x == 0 || pen.y == 0 {
        return Ok(());
    }
    let (mut x, mut y) = (from.x, from.y);
    let (dx, dy) = ((to.x - from.x).abs(), -(to.y - from.y).abs());
    let (sx, sy) = (
        if from.x < to.x { 1 } else { -1 },
        if from.y < to.y { 1 } else { -1 },
    );
    let mut error = dx + dy;
    loop {
        budget.work(1)?;
        let stamp = Rect {
            top: y,
            left: x,
            bottom: y + pen.y,
            right: x + pen.x,
        }
        .intersect(frame);
        budget.work((stamp.width() * stamp.height()) as u64)?;
        if stamp.width() != 0 {
            for py in stamp.top..stamp.bottom {
                for px in stamp.left..stamp.right {
                    mask[(py - frame.top) as usize * frame.width() + (px - frame.left) as usize] =
                        1;
                }
            }
        }
        if x == to.x && y == to.y {
            break;
        }
        let twice = 2 * error;
        if twice >= dy {
            error += dy;
            x += sx;
        }
        if twice <= dx {
            error += dx;
            y += sy;
        }
    }
    Ok(())
}
fn apply_mask(
    canvas: &mut Canvas,
    mask: &[u8],
    state: &State<'_>,
    budget: &mut Budget<'_>,
) -> Result<()> {
    budget.work(mask.len() as u64)?;
    for (i, &covered) in mask.iter().enumerate() {
        if covered != 0 {
            canvas.patterned(
                canvas.frame.left + (i % canvas.frame.width()) as i32,
                canvas.frame.top + (i / canvas.frame.width()) as i32,
                Verb::Paint,
                state,
            );
        }
    }
    Ok(())
}
fn line(
    canvas: &mut Canvas,
    from: Point,
    to: Point,
    state: &State<'_>,
    budget: &mut Budget<'_>,
) -> Result<()> {
    if state.pen.x == 0 || state.pen.y == 0 {
        return Ok(());
    }
    let mut mask = coverage_mask(canvas, budget)?;
    stamp_line(&mut mask, canvas.frame, from, to, state.pen, budget)?;
    apply_mask(canvas, &mask, state, budget)
}

fn blit(
    canvas: &mut Canvas,
    bitmap: &Bitmap,
    state: &State<'_>,
    budget: &mut Budget<'_>,
) -> Result<()> {
    let dest = bitmap.destination;
    let area = dest.intersect(canvas.frame);
    if area.width() == 0 || area.height() == 0 {
        return Ok(());
    }
    budget.work((area.width() * area.height()) as u64)?;
    for y in area.top..area.bottom {
        for x in area.left..area.right {
            if bitmap
                .mask
                .as_ref()
                .is_some_and(|mask| !mask.contains(x, y))
            {
                continue;
            }
            let sx = bitmap.source.left
                + (((x - dest.left) as i64 * bitmap.source.width() as i64) / dest.width() as i64)
                    as i32;
            let sy = bitmap.source.top
                + (((y - dest.top) as i64 * bitmap.source.height() as i64) / dest.height() as i64)
                    as i32;
            let index = (sy - bitmap.bounds.top) as usize * bitmap.bounds.width()
                + (sx - bitmap.bounds.left) as usize;
            let color = match &bitmap.pixels {
                Pixels::Mono(pixels) => {
                    if pixels[index] == 0 {
                        state.background
                    } else {
                        state.foreground
                    }
                }
                Pixels::Rgb(pixels) => [
                    pixels[index * 3],
                    pixels[index * 3 + 1],
                    pixels[index * 3 + 2],
                ],
            };
            canvas.apply(x, y, color, bitmap.mode, state);
        }
    }
    Ok(())
}
