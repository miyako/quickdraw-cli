use crate::model::*;

#[derive(Clone, Debug)]
pub struct RenderOptions {
    pub fidelity: Fidelity,
    pub printer_comments: crate::PrinterComments,
    pub background: [u8; 3],
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub limits: Limits,
    pub highlight: Option<[u8; 3]>,
    pub fonts: crate::FontBook,
}
impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            fidelity: Fidelity::Strict,
            printer_comments: crate::PrinterComments::Ignore,
            background: [255; 3],
            width: None,
            height: None,
            limits: Limits::default(),
            highlight: None,
            fonts: crate::FontBook::default(),
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

pub(crate) struct State<'a> {
    pub printer: crate::printer::PrinterState,
    pub clip: Option<&'a Region>,
    pub foreground: [u8; 3],
    pub background: [u8; 3],
    pub pen_pattern: &'a Pattern,
    pub background_pattern: &'a Pattern,
    pub fill_pattern: &'a Pattern,
    pub pen: Point,
    pub mode: u16,
    pub origin: Point,
    pub oval: Point,
    pub op_color: Option<[u16; 3]>,
    pub highlight_color: Option<[u8; 3]>,
    pub highlight: bool,
    pub text: crate::font::TextState,
    pub pen_location: Point,
    pub text_start: Point,
}
impl Default for State<'_> {
    fn default() -> Self {
        Self {
            printer: Default::default(),
            clip: None,
            foreground: [0; 3],
            background: [255; 3],
            pen_pattern: &Pattern::Bits([255; 8]),
            background_pattern: &Pattern::Bits([0; 8]),
            fill_pattern: &Pattern::Bits([255; 8]),
            pen: Point { x: 1, y: 1 },
            mode: 8,
            origin: Point { x: 0, y: 0 },
            oval: Point { x: 0, y: 0 },
            op_color: None,
            highlight_color: None,
            highlight: false,
            text: crate::font::TextState::new(),
            pen_location: Point { x: 0, y: 0 },
            text_start: Point { x: 0, y: 0 },
        }
    }
}
impl<'a> State<'a> {
    pub fn update(&mut self, command: &'a Command, options: &RenderOptions) -> Result<bool> {
        if options.printer_comments == crate::PrinterComments::ApplePortable
            && self.printer.intercept(command)?
        {
            return Ok(true);
        }
        match command {
            Command::Nop | Command::Metadata(_) => {}
            Command::Clip(region) => self.clip = Some(region),
            Command::BackgroundPattern(p) => self.background_pattern = p,
            Command::PenPattern(p) => self.pen_pattern = p,
            Command::FillPattern(p) => self.fill_pattern = p,
            Command::Foreground(c) => self.foreground = *c,
            Command::Background(c) => self.background = *c,
            Command::PenSize(p) => self.pen = *p,
            Command::PenMode(mode) => self.mode = *mode,
            Command::Origin(delta) => {
                let add = |a: i32, b: i32| {
                    a.checked_add(b)
                        .ok_or_else(|| Error::new(ErrorCode::ResourceLimit, "origin overflow"))
                };
                self.origin = Point {
                    x: add(self.origin.x, delta.x)?,
                    y: add(self.origin.y, delta.y)?,
                };
            }
            Command::OvalSize(size) => self.oval = *size,
            Command::OpColor(color) => self.op_color = Some(*color),
            Command::HighlightColor(color) => self.highlight_color = color.or(options.highlight),
            Command::Highlight => self.highlight = true,
            Command::TextState { opcode, data } => self.text.update(*opcode, data)?,
            _ => return Ok(false),
        }
        Ok(true)
    }
    pub fn clip(&self) -> Option<&'a Region> {
        if self.printer.rotating() {
            None
        } else {
            self.clip
        }
    }
    pub fn drawing(&self, verb: Verb) -> (&Pattern, u16) {
        match verb {
            Verb::Frame | Verb::Paint => (self.pen_pattern, self.mode),
            Verb::Erase => (self.background_pattern, 8),
            Verb::Fill => (self.fill_pattern, 8),
            Verb::Invert => (&Pattern::Bits([255; 8]), 2),
        }
    }
    fn validate(&self, mode: u16, pattern: bool) -> Result<()> {
        check_mode(mode, pattern)?;
        if matches!(mode, 32 | 33 | 35) && self.op_color.is_none() {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "blend/addPin/subPin requires explicit OpColor",
            ));
        }
        if self.is_highlight(mode) && self.highlight_color.is_none() {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "system highlight color is unknown; specify --highlight or HiliteColor",
            ));
        }
        Ok(())
    }
    fn is_highlight(&self, mode: u16) -> bool {
        matches!(mode, 50 | 52 | 58 | 60) || (self.highlight && matches!(mode, 2 | 10))
    }
}

struct Canvas {
    frame: Rect,
    rgb: Vec<u8>,
}
impl Canvas {
    fn composite(&mut self, x: i32, y: i32, rgb: [u8; 3], alpha: u8, mode: u16, state: &State<'_>) {
        if !self.frame.contains(x, y) || state.clip().is_some_and(|c| !c.contains(x, y)) {
            return;
        }
        let index = ((y - self.frame.top) as usize * self.frame.width()
            + (x - self.frame.left) as usize)
            * 3;
        for (c, (dest, source)) in self.rgb[index..index + 3].iter_mut().zip(rgb).enumerate() {
            if mode == 32 {
                *dest = arithmetic(source, *dest, mode, state.op_color.map(|v| v[c]));
                continue;
            }
            if mode == 260 {
                let denominator = 255u64 * 65535;
                let weight = u64::from(alpha)
                    * u64::from(state.op_color.expect("validated image OpColor")[c]);
                *dest = ((u64::from(source) * weight
                    + u64::from(*dest) * (denominator - weight)
                    + denominator / 2)
                    / denominator) as u8;
                continue;
            }
            let a = i32::from(alpha);
            let foreground = match mode {
                256 => i32::from(source) * a,
                257 => (i32::from(source) - (255 - a)) * 255,
                258 => i32::from(source) * 255,
                _ => unreachable!("validated alpha mode"),
            };
            *dest = ((foreground + i32::from(*dest) * (255 - a) + 127) / 255) as u8;
        }
    }
    fn store(&mut self, x: i32, y: i32, rgb: [u8; 3], state: &State<'_>) {
        if self.frame.contains(x, y) && state.clip().is_none_or(|c| c.contains(x, y)) {
            let i = ((y - self.frame.top) as usize * self.frame.width()
                + (x - self.frame.left) as usize)
                * 3;
            self.rgb[i..i + 3].copy_from_slice(&rgb);
        }
    }
    fn apply(&mut self, x: i32, y: i32, source: [u8; 3], mode: u16, state: &State<'_>) {
        if !self.frame.contains(x, y) || state.clip().is_some_and(|clip| !clip.contains(x, y)) {
            return;
        }
        let index = ((y - self.frame.top) as usize * self.frame.width()
            + (x - self.frame.left) as usize)
            * 3;
        let dest = &mut self.rgb[index..index + 3];
        if state.is_highlight(mode) {
            if source != [255; 3] {
                let highlight = state.highlight_color.expect("validated highlight color");
                if dest == state.background {
                    dest.copy_from_slice(&highlight);
                } else if dest == highlight {
                    dest.copy_from_slice(&state.background);
                }
            }
            return;
        }
        if matches!(mode, 36 | 44) && source == state.background {
            return;
        }
        for c in 0..3 {
            dest[c] = if mode >= 32 {
                arithmetic(source[c], dest[c], mode, state.op_color.map(|v| v[c]))
            } else {
                let mode = mode & 7;
                let src = if mode & 4 != 0 { !source[c] } else { source[c] };
                let fg = state.foreground[c];
                let bg = state.background[c];
                match mode & 3 {
                    0 => copy_component(src, fg, bg),
                    1 => (src | fg) & dest[c],
                    2 => {
                        if source == [if mode & 4 == 0 { 0 } else { 255 }; 3] {
                            !dest[c]
                        } else {
                            dest[c]
                        }
                    }
                    _ => (!src & bg) | dest[c],
                }
            };
        }
    }
    fn patterned(&mut self, x: i32, y: i32, verb: Verb, state: &State<'_>) {
        let (pattern, mode) = state.drawing(verb);
        let color = pattern.sample(x - state.origin.x, y - state.origin.y);
        if matches!(pattern, Pattern::Bits(_)) {
            self.apply(x, y, color, mode, state);
        } else if self.frame.contains(x, y) && state.clip().is_none_or(|clip| clip.contains(x, y)) {
            let i = ((y - self.frame.top) as usize * self.frame.width()
                + (x - self.frame.left) as usize)
                * 3;
            self.rgb[i..i + 3].copy_from_slice(&color);
        }
    }
}

fn copy_component(source: u8, fg: u8, bg: u8) -> u8 {
    ((u32::from(255 - source) * u32::from(fg) + u32::from(source) * u32::from(bg)) / 255) as u8
}
pub(crate) fn copy_color(source: [u8; 3], state: &State<'_>) -> [u8; 3] {
    std::array::from_fn(|i| copy_component(source[i], state.foreground[i], state.background[i]))
}

// QuickDraw monochrome logical ink uses one for black; RGB uses zero for black.
fn arithmetic(source: u8, dest: u8, mode: u16, op: Option<u16>) -> u8 {
    match mode {
        32 => {
            let weight = u64::from(op.expect("validated OpColor"));
            ((u64::from(source) * weight + u64::from(dest) * (65535 - weight)) / 65535) as u8
        }
        33 => (u16::from(source) + u16::from(dest)).min(op.expect("validated OpColor") >> 8) as u8,
        34 => source.wrapping_add(dest),
        35 => dest
            .saturating_sub(source)
            .max((op.expect("validated OpColor") >> 8) as u8),
        36 | 44 => source,
        37 => source.max(dest),
        38 => dest.wrapping_sub(source),
        39 => source.min(dest),
        _ => unreachable!("validated transfer mode"),
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
    if (pattern && ((8..=15).contains(&mode) || mode == 44))
        || (!pattern && mode <= 7)
        || matches!(mode, 32..=39 | 50 | 52 | 58 | 60)
    {
        Ok(())
    } else {
        Err(Error::new(
            ErrorCode::Unsupported,
            format!("unsupported transfer mode {mode}"),
        ))
    }
}

pub(crate) fn output_size(frame: Rect, options: &RenderOptions) -> Result<(usize, usize)> {
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
    let mut state = State {
        highlight_color: options.highlight,
        ..Default::default()
    };
    let mut diagnostics = vec![Diagnostic::new(
        "unverified_fidelity",
        "Portable RGB raster profile; no classic Mac rendering oracle or historical fonts were used to validate pixel identity.",
        None,
    )];
    crate::printer::policy(options, &mut diagnostics)?;
    for op in &picture.operations {
        let result = (|| {
            budget.work(1)?;
            if state.update(&op.command, options)? {
                canvas.frame = frame.translated(state.origin)?;
                return Ok(());
            }
            match &op.command {
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
                Command::Line(_, _) | Command::LineFrom(_) | Command::ShortLineFrom(_) => {
                    let (from, to) = line_points(&op.command, &state)?;
                    if matches!(state.pen_pattern, Pattern::Bits(_)) {
                        state.validate(state.mode, true)?;
                    }
                    if from.x != to.x && from.y != to.y {
                        require_approximate(
                            options,
                            &mut diagnostics,
                            op.offset,
                            "diagonal line uses integer Bresenham coverage",
                        )?;
                    }
                    line(&mut canvas, from, to, &state, &mut budget)?;
                    state.pen_location = to;
                    state.highlight = false;
                }
                Command::Text { .. } => {
                    text_policy(&state, options, &mut diagnostics, op.offset)?;
                    let run = prepare_text(&mut state, &op.command, options, &mut budget)?;
                    let mode = state.text.base_mode();
                    state.validate(mode, false)?;
                    let mask = run.mask(canvas.frame, state.text.masked(), &mut budget)?;
                    for y in mask.bounds.top..mask.bounds.bottom {
                        for x in mask.bounds.left..mask.bounds.right {
                            let ink = mask.ink(x, y);
                            if !ink && state.text.masked() {
                                continue;
                            }
                            let source = if mode >= 32 {
                                if ink {
                                    state.foreground
                                } else {
                                    state.background
                                }
                            } else {
                                [if ink { 0 } else { 255 }; 3]
                            };
                            canvas.apply(x, y, source, mode, &state);
                        }
                    }
                    diagnostics.push(font_diagnostic(&state, options, op.offset));
                    state.highlight = false;
                }
                Command::Draw(verb, shape) => {
                    let (pattern, mode) = state.drawing(*verb);
                    if matches!(pattern, Pattern::Bits(_)) {
                        state.validate(mode, !matches!(verb, Verb::Invert))?;
                    }
                    if matches!(
                        shape,
                        Shape::Oval(_)
                            | Shape::RoundRect(_)
                            | Shape::Arc { .. }
                            | Shape::Polygon { .. }
                    ) || matches!(
                        (verb, shape),
                        (Verb::Frame, Shape::Region(Region { mask: Some(_), .. }))
                    ) {
                        require_approximate(
                            options,
                            &mut diagnostics,
                            op.offset,
                            "curve/polygon/complex region outline uses portable coverage",
                        )?;
                    }
                    draw(&mut canvas, *verb, shape, &state, &mut budget)?;
                    state.highlight = false;
                }
                Command::Bitmap(bitmap) => {
                    if bitmap.raw_copy {
                        image_policy(bitmap, &state, options, &mut diagnostics, op.offset)?;
                    } else {
                        copybits_policy(bitmap, options, &mut diagnostics, op.offset)?;
                        state.validate(bitmap.transfer_mode(), false)?;
                    }
                    if matches!(bitmap.pixels, Pixels::Rgb(_))
                        && (1..=7).contains(&bitmap.transfer_mode())
                    {
                        require_approximate(
                            options,
                            &mut diagnostics,
                            op.offset,
                            "color bitmap Boolean mode uses RGB ink-space logic",
                        )?;
                    }
                    blit(&mut canvas, bitmap, &state, &mut budget)?;
                    state.highlight = false;
                }
                _ => unreachable!("state command already handled"),
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
    state.printer.finish()?;
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

pub(crate) fn line_points(command: &Command, state: &State<'_>) -> Result<(Point, Point)> {
    let from = state.pen_location;
    Ok(match command {
        Command::Line(from, to) => (*from, *to),
        Command::LineFrom(to) => (from, *to),
        Command::ShortLineFrom(delta) => {
            let x = from.x.checked_add(delta.x);
            let y = from.y.checked_add(delta.y);
            let (Some(x), Some(y)) = (x, y) else {
                return Err(Error::new(
                    ErrorCode::ResourceLimit,
                    "line coordinate overflow",
                ));
            };
            if i16::try_from(x).is_err() || i16::try_from(y).is_err() {
                return Err(Error::new(
                    ErrorCode::Malformed,
                    "short line coordinate overflow",
                ));
            }
            (from, Point { x, y })
        }
        _ => unreachable!("line command"),
    })
}

pub(crate) struct PlacedGlyph<'a> {
    pub glyph: crate::font::Glyph<'a>,
    pub bounds: Rect,
}

pub(crate) struct TextRun<'a> {
    pub outline: Option<crate::outline::OutlineRun>,
    glyphs: Vec<PlacedGlyph<'a>>,
    bounds: Rect,
    advance_right: i32,
}

pub(crate) struct TextMask {
    pub bounds: Rect,
    pub(crate) ink: Vec<u8>,
}
impl TextMask {
    pub fn ink(&self, x: i32, y: i32) -> bool {
        self.ink
            [(y - self.bounds.top) as usize * self.bounds.width() + (x - self.bounds.left) as usize]
            != 0
    }
}
impl TextRun<'_> {
    pub fn mask(&self, viewport: Rect, masked: bool, budget: &mut Budget<'_>) -> Result<TextMask> {
        if let Some(outline) = &self.outline {
            return outline.mask(viewport, masked, budget);
        }
        let mut bounds = self.bounds.intersect(viewport);
        if masked {
            bounds.right = bounds.right.min(self.advance_right);
        }
        if bounds.width() == 0 || bounds.height() == 0 {
            return Ok(TextMask {
                bounds: Rect {
                    right: bounds.left,
                    bottom: bounds.top,
                    ..bounds
                },
                ink: Vec::new(),
            });
        }
        let count = bounds
            .width()
            .checked_mul(bounds.height())
            .ok_or_else(|| Error::new(ErrorCode::ResourceLimit, "text mask size overflow"))?;
        budget.allocate(count)?;
        budget.work(count as u64)?;
        let mut mask = TextMask {
            bounds,
            ink: vec![0; count],
        };
        for placed in &self.glyphs {
            let area = placed.bounds.intersect(bounds);
            if area.width() == 0 || area.height() == 0 {
                continue;
            }
            budget.work((area.width() * area.height()) as u64)?;
            for y in area.top..area.bottom {
                for x in area.left..area.right {
                    let sx = (i64::from(x) - i64::from(placed.bounds.left)) as usize
                        * placed.glyph.width
                        / placed.bounds.width();
                    let sy = (i64::from(y) - i64::from(placed.bounds.top)) as usize
                        * placed.glyph.height
                        / placed.bounds.height();
                    if placed.glyph.ink(sx, sy) {
                        let i =
                            (y - bounds.top) as usize * bounds.width() + (x - bounds.left) as usize;
                        mask.ink[i] = 1;
                    }
                }
            }
        }
        Ok(mask)
    }
}

pub(crate) fn text_policy(
    state: &State<'_>,
    options: &RenderOptions,
    diagnostics: &mut Vec<Diagnostic>,
    offset: usize,
) -> Result<()> {
    let outline = options
        .fonts
        .outline(state.text.id, state.text.size, state.text.face)
        .is_some();
    state.text.check_for(outline)?;
    if outline {
        require_approximate(
            options,
            diagnostics,
            offset,
            "portable_outline_v1: explicit MacRoman/static TrueType shaping, unhinted outline coverage; not classic Font Manager mapping or device fidelity",
        )?;
    }
    if !outline && (state.text.portable_layout() || !matches!(state.text.mode, 1 | 3)) {
        require_approximate(
            options,
            diagnostics,
            offset,
            "text uses portable fixed-point layout/nearest-neighbor scaling or whole-run compositing, not historical Font Manager/stack-dependent rendering",
        )?;
    }
    if options
        .fonts
        .get(state.text.id, state.text.size, state.text.face)
        .is_ok_and(|font| font.kerning_enabled())
    {
        require_approximate(
            options,
            diagnostics,
            offset,
            "explicit FOND pair kerning is applied within each text run; this is an opt-in layout policy, not inferred QuickDraw behavior",
        )?;
    }
    Ok(())
}

pub(crate) fn font_diagnostic(
    state: &State<'_>,
    options: &RenderOptions,
    offset: usize,
) -> Diagnostic {
    let outline = options
        .fonts
        .outline(state.text.id, state.text.size, state.text.face)
        .is_some();
    Diagnostic::new(
        if outline {
            "explicit_outline_font"
        } else {
            "explicit_bitmap_font"
        },
        format!(
            "{} id={}, size={}, face={}; exact caller-supplied binding, no substitution",
            if outline {
                "portable_outline_v1 MacRoman"
            } else {
                "NFNT"
            },
            state.text.id,
            state.text.size,
            state.text.face
        ),
        Some(offset),
    )
}

pub(crate) fn prepare_text<'a>(
    state: &mut State<'_>,
    command: &Command,
    options: &'a RenderOptions,
    budget: &mut Budget<'_>,
) -> Result<TextRun<'a>> {
    let mut run = layout_text(state, command, options, budget)?;
    let Some(transform) = state.printer.transform(state.text_start)? else {
        return Ok(run);
    };
    if state.text.masked()
        || !matches!(state.text.base_mode(), 1 | 2 | 3 | 36)
        || (state.text.base_mode() != 36
            && (state.foreground != [0; 3] || state.background != [255; 3]))
    {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "rotated printer text requires unmasked black/white srcOr/srcXor/srcBic or transparent text; opaque/background and masked-run geometry are outside this profile",
        ));
    }
    use crate::outline::{OutlineRun, OutlineSegment as S};
    let mut outline = if let Some(outline) = run.outline.take() {
        outline
    } else {
        let mask = run.mask(run.bounds, false, budget)?;
        let mut segments = Vec::new();
        budget.work(mask.ink.len() as u64)?;
        for y in mask.bounds.top..mask.bounds.bottom {
            let mut x = mask.bounds.left;
            while x < mask.bounds.right {
                if !mask.ink(x, y) {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < mask.bounds.right && mask.ink(x, y) {
                    x += 1;
                }
                if segments.len() + 5 > budget.limits.max_operations {
                    return Err(Error::new(
                        ErrorCode::ResourceLimit,
                        "rotated bitmap text path limit exceeded",
                    ));
                }
                budget.allocate(5 * std::mem::size_of::<S>())?;
                segments.extend([
                    S::Move([f64::from(start), f64::from(y)]),
                    S::Line([f64::from(x), f64::from(y)]),
                    S::Line([f64::from(x), f64::from(y) + 1.0]),
                    S::Line([f64::from(start), f64::from(y) + 1.0]),
                    S::Close,
                ]);
            }
        }
        OutlineRun {
            glyphs: vec![segments],
            bounds: run.bounds,
            advance_right: run.advance_right,
        }
    };
    let r = outline.bounds;
    let corners = [
        [f64::from(r.left), f64::from(r.top)],
        [f64::from(r.right), f64::from(r.top)],
        [f64::from(r.right), f64::from(r.bottom)],
        [f64::from(r.left), f64::from(r.bottom)],
    ]
    .map(|p| transform.point(p));
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in corners {
        bounds[0] = bounds[0].min(p[0]);
        bounds[1] = bounds[1].min(p[1]);
        bounds[2] = bounds[2].max(p[0]);
        bounds[3] = bounds[3].max(p[1]);
    }
    outline.bounds = crate::printer::bounds(bounds)?;
    budget
        .limits
        .pixels(outline.bounds.width(), outline.bounds.height())?;
    outline.advance_right = outline.bounds.right;
    for glyph in &mut outline.glyphs {
        for segment in glyph {
            budget.work(1)?;
            match segment {
                S::Move(p) | S::Line(p) => *p = transform.point(*p),
                S::Quad(points) => points.iter_mut().for_each(|p| *p = transform.point(*p)),
                S::Curve(points) => points.iter_mut().for_each(|p| *p = transform.point(*p)),
                S::Close => {}
            }
        }
    }
    run.outline = Some(outline);
    Ok(run)
}

fn layout_text<'a>(
    state: &mut State<'_>,
    command: &Command,
    options: &'a RenderOptions,
    budget: &mut Budget<'_>,
) -> Result<TextRun<'a>> {
    let Command::Text {
        position,
        delta,
        bytes,
    } = command
    else {
        unreachable!("text command")
    };
    let outline = options
        .fonts
        .outline(state.text.id, state.text.size, state.text.face);
    state.text.check_for(outline.is_some())?;
    let add = |a: i32, b: i32| {
        a.checked_add(b)
            .ok_or_else(|| Error::new(ErrorCode::ResourceLimit, "text coordinate overflow"))
    };
    let baseline = match position {
        Some(point) => *point,
        None => Point {
            x: add(state.text_start.x, delta.x)?,
            y: add(state.text_start.y, delta.y)?,
        },
    };
    state.text_start = baseline;
    let pixel = |fixed: i64| {
        i32::try_from(fixed.div_euclid(65536))
            .map_err(|_| Error::new(ErrorCode::ResourceLimit, "text coordinate overflow"))
    };
    let mut x = i64::from(baseline.x) * 65536 + i64::from(state.text.fraction);
    let mut bounds = Rect {
        left: baseline.x,
        right: baseline.x,
        top: baseline.y,
        bottom: baseline.y,
    };
    if bytes.is_empty() {
        state.pen_location = baseline;
        return Ok(TextRun {
            outline: None,
            glyphs: Vec::new(),
            bounds,
            advance_right: baseline.x,
        });
    }
    if let Some(font) = outline {
        let run = font.layout(bytes, baseline, &state.text, budget)?;
        state.pen_location = Point {
            x: run.advance_right,
            y: baseline.y,
        };
        return Ok(TextRun {
            bounds: run.bounds,
            advance_right: run.advance_right,
            glyphs: Vec::new(),
            outline: Some(run),
        });
    }
    let font = options
        .fonts
        .get(state.text.id, state.text.size, state.text.face)?;
    budget.allocate(bytes.len() * std::mem::size_of::<PlacedGlyph<'_>>())?;
    let mut glyphs = Vec::with_capacity(bytes.len());
    for (index, &code) in bytes.iter().enumerate() {
        if index != 0 {
            x = x
                .checked_add(state.text.scale(font.kern(bytes[index - 1], code), true)?)
                .ok_or_else(|| Error::new(ErrorCode::ResourceLimit, "text kerning overflow"))?;
        }
        let glyph = font.glyph(code, state.text.fractional_widths)?;
        let left_fixed = x + state.text.scale(i64::from(glyph.bearing) * 65536, true)?;
        let top_fixed = i64::from(baseline.y) * 65536 + 0x8000
            - state.text.scale(i64::from(glyph.ascent) * 65536, false)?;
        let placed = Rect {
            left: pixel(left_fixed)?,
            top: pixel(top_fixed)?,
            right: pixel(left_fixed + state.text.scale(glyph.width as i64 * 65536, true)?)?,
            bottom: pixel(top_fixed + state.text.scale(glyph.height as i64 * 65536, false)?)?,
        };
        if placed.width() != 0 && placed.height() != 0 {
            options.limits.pixels(placed.width(), placed.height())?;
        }
        budget.work((placed.width() * placed.height() + 1) as u64)?;
        let advance = if state.text.fractional_widths {
            glyph
                .fractional_advance
                .unwrap_or(i64::from(glyph.advance) * 65536)
        } else {
            i64::from(glyph.advance) * 65536
        };
        x = x
            .checked_add(state.text.scale(advance + state.text.spacing(code), true)?)
            .ok_or_else(|| Error::new(ErrorCode::ResourceLimit, "text advance overflow"))?;
        let pen = pixel(x)?;
        bounds.left = bounds.left.min(placed.left).min(pen);
        bounds.right = bounds.right.max(placed.right).max(pen);
        bounds.top = bounds.top.min(placed.top);
        bounds.bottom = bounds.bottom.max(placed.bottom);
        glyphs.push(PlacedGlyph {
            glyph,
            bounds: placed,
        });
    }
    if bounds.width() != 0 && bounds.height() != 0 {
        options.limits.pixels(bounds.width(), bounds.height())?;
    }
    state.pen_location = Point {
        x: pixel(x)?,
        y: baseline.y,
    };
    Ok(TextRun {
        outline: None,
        glyphs,
        bounds,
        advance_right: state.pen_location.x,
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

fn round_contains(rect: Rect, size: Point, x: i32, y: i32) -> bool {
    if !rect.contains(x, y) {
        return false;
    }
    let w = size.x.min(rect.width() as i32);
    let h = size.y.min(rect.height() as i32);
    if w <= 0 || h <= 0 {
        return true;
    }
    let px = 2 * (x - rect.left) + 1;
    let py = 2 * (y - rect.top) + 1;
    if (px >= w && px <= 2 * rect.width() as i32 - w)
        || (py >= h && py <= 2 * rect.height() as i32 - h)
    {
        return true;
    }
    let left = if px < w { rect.left } else { rect.right - w };
    let top = if py < h { rect.top } else { rect.bottom - h };
    oval_contains(
        Rect {
            left,
            top,
            right: left + w,
            bottom: top + h,
        },
        x,
        y,
    )
}

fn arc_contains(rect: Rect, start: i32, sweep: i32, x: i32, y: i32) -> bool {
    if sweep.abs() >= 360 {
        return true;
    }
    if sweep == 0 {
        return false;
    }
    let (start, extent) = if sweep < 0 {
        (start + sweep, -sweep)
    } else {
        (start, sweep)
    };
    let direction = |angle: i32| {
        let angle = (angle.rem_euclid(360) as f64).to_radians();
        (
            (angle.sin() * 1_000_000_000.0).round() as i128,
            (-angle.cos() * 1_000_000_000.0).round() as i128,
        )
    };
    let a = direction(start);
    let b = direction(start + extent);
    let px = (2 * x - rect.left - rect.right + 1) as i128 * rect.height() as i128;
    let py = (2 * y - rect.top - rect.bottom + 1) as i128 * rect.width() as i128;
    if px == 0 && py == 0 {
        return true;
    }
    let after_start = a.0 * py - a.1 * px >= 0;
    let before_end = px * b.1 - py * b.0 > 0;
    if extent <= 180 {
        after_start && before_end
    } else {
        after_start || before_end
    }
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
    visit_shape(canvas.frame, verb, shape, state, budget, |x, y| {
        canvas.patterned(x, y, verb, state);
    })
}

fn visit_shape(
    frame: Rect,
    verb: Verb,
    shape: &Shape,
    state: &State<'_>,
    budget: &mut Budget<'_>,
    mut emit: impl FnMut(i32, i32),
) -> Result<()> {
    if matches!(verb, Verb::Frame) && (state.pen.x == 0 || state.pen.y == 0) {
        return Ok(());
    }
    if let (Verb::Frame, Shape::Polygon { points, .. }) = (verb, shape) {
        let mut mask = coverage_mask(frame, budget)?;
        for (a, b) in points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .take(points.len())
        {
            stamp_line(&mut mask, frame, *a, *b, state.pen, budget)?;
        }
        budget.work(mask.len() as u64)?;
        for (i, &covered) in mask.iter().enumerate() {
            if covered != 0 {
                emit(
                    frame.left + (i % frame.width()) as i32,
                    frame.top + (i / frame.width()) as i32,
                );
            }
        }
        return Ok(());
    }
    let bounds = match shape {
        Shape::Rect(r) | Shape::Oval(r) | Shape::RoundRect(r) => *r,
        Shape::Arc { bounds, .. } => *bounds,
        Shape::Polygon { bounds, .. } => *bounds,
        Shape::Region(region) => region.bounds,
    };
    let area = bounds.intersect(frame);
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
                Shape::RoundRect(rect) => round_contains(*rect, state.oval, x, y),
                Shape::Arc {
                    bounds,
                    start,
                    sweep,
                } => oval_contains(*bounds, x, y) && arc_contains(*bounds, *start, *sweep, x, y),
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
                    Shape::RoundRect(rect) => round_contains(
                        rect.inset(state.pen.x, state.pen.y),
                        Point {
                            x: (state.oval.x.min(rect.width() as i32) - 2 * state.pen.x).max(0),
                            y: (state.oval.y.min(rect.height() as i32) - 2 * state.pen.y).max(0),
                        },
                        x,
                        y,
                    ),
                    Shape::Arc { bounds, .. } => {
                        oval_contains(bounds.inset(state.pen.x, state.pen.y), x, y)
                    }
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
            emit(x, y);
        }
    }
    Ok(())
}

fn coverage_mask(frame: Rect, budget: &mut Budget<'_>) -> Result<Vec<u8>> {
    let count = budget.limits.pixels(frame.width(), frame.height())?;
    // Cumulative allocation budget also bounds repeated full-frame temporary masks.
    budget.allocate(count)?;
    budget.work(count as u64)?;
    Ok(vec![0; count])
}

pub(crate) fn frame_coverage(
    frame: Rect,
    shape: &Shape,
    state: &State<'_>,
    budget: &mut Budget<'_>,
) -> Result<Vec<u8>> {
    let mut mask = coverage_mask(frame, budget)?;
    visit_shape(frame, Verb::Frame, shape, state, budget, |x, y| {
        mask[(y - frame.top) as usize * frame.width() + (x - frame.left) as usize] = 1;
    })?;
    Ok(mask)
}

pub(crate) fn line_coverage(
    frame: Rect,
    from: Point,
    to: Point,
    pen: Point,
    budget: &mut Budget<'_>,
) -> Result<Vec<u8>> {
    let mut mask = coverage_mask(frame, budget)?;
    stamp_line(&mut mask, frame, from, to, pen, budget)?;
    Ok(mask)
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
    let (mut x, mut y) = (i64::from(from.x), i64::from(from.y));
    let (tx, ty) = (i64::from(to.x), i64::from(to.y));
    let (dx, dy) = ((tx - x).abs(), -(ty - y).abs());
    let (sx, sy) = (
        if from.x < to.x { 1 } else { -1 },
        if from.y < to.y { 1 } else { -1 },
    );
    let mut error = dx + dy;
    loop {
        budget.work(1)?;
        let stamp = Rect {
            top: y.clamp(i64::from(frame.top), i64::from(frame.bottom)) as i32,
            left: x.clamp(i64::from(frame.left), i64::from(frame.right)) as i32,
            bottom: (y + i64::from(pen.y)).clamp(i64::from(frame.top), i64::from(frame.bottom))
                as i32,
            right: (x + i64::from(pen.x)).clamp(i64::from(frame.left), i64::from(frame.right))
                as i32,
        };
        budget.work((stamp.width() * stamp.height()) as u64)?;
        if stamp.width() != 0 {
            for py in stamp.top..stamp.bottom {
                for px in stamp.left..stamp.right {
                    mask[(py - frame.top) as usize * frame.width() + (px - frame.left) as usize] =
                        1;
                }
            }
        }
        if x == tx && y == ty {
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
    if let Some(rect) = state.printer.line(from, to, state.pen)? {
        let area = crate::printer::bounds(rect)?.intersect(canvas.frame);
        budget.work((area.width() * area.height()) as u64)?;
        for y in area.top..area.bottom {
            for x in area.left..area.right {
                if f64::from(x) + 0.5 >= rect[0]
                    && f64::from(x) + 0.5 < rect[2]
                    && f64::from(y) + 0.5 >= rect[1]
                    && f64::from(y) + 0.5 < rect[3]
                {
                    canvas.patterned(x, y, Verb::Paint, state);
                }
            }
        }
        return Ok(());
    }
    let mask = line_coverage(canvas.frame, from, to, state.pen, budget)?;
    apply_mask(canvas, &mask, state, budget)
}

pub(crate) fn copybits_policy(
    bitmap: &Bitmap,
    options: &RenderOptions,
    diagnostics: &mut Vec<Diagnostic>,
    offset: usize,
) -> Result<()> {
    if bitmap.transfer_mode() != bitmap.mode {
        require_approximate(
            options,
            diagnostics,
            offset,
            "CopyBits dither hint is inactive only in the no-reduction portable RGB8 profile",
        )?;
        diagnostics.push(Diagnostic::new("copybits_dither_inactive",
            "CopyBits bit 64 accepted: source precision fits RGB8 and neither dimension shrinks. No palette quantization, error diffusion or averaging is performed.",
            Some(offset)));
    }
    Ok(())
}

pub(crate) fn image_policy(
    bitmap: &Bitmap,
    state: &State<'_>,
    options: &RenderOptions,
    diagnostics: &mut Vec<Diagnostic>,
    offset: usize,
) -> Result<()> {
    if matches!(bitmap.mode, 32 | 36 | 260) && state.op_color.is_none() {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "QuickTime image blend/transparency key requires explicit OpColor",
        ));
    }
    diagnostics.push(Diagnostic::new("embedded_still_image",
        "Embedded still decoded with explicit transfer mode; no historical codec/color-profile fidelity is claimed.", Some(offset)));
    if let Some((at, _)) = &bitmap.description_extensions {
        diagnostics.push(Diagnostic::new("image_description_extensions","Validated neutral description extensions/terminator are retained by inspection; no rendering-affecting extension is ignored.",Some(*at)));
    }
    if bitmap.affine.is_some() || bitmap.mode != 0 {
        require_approximate(
            options,
            diagnostics,
            offset,
            "QuickTime uses portable inverse pixel-center affine sampling and/or quantized OpColor/alpha composition, not historical device rendering",
        )?;
    }
    Ok(())
}

pub(crate) fn transparent_pixel(bitmap: &Bitmap, source: [u8; 3], state: &State<'_>) -> bool {
    bitmap.mode == 36
        && source
            == if bitmap.raw_copy {
                state
                    .op_color
                    .expect("validated image OpColor")
                    .map(|v| (v >> 8) as u8)
            } else {
                state.background
            }
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
            if let Some((color, alpha)) = bitmap.sample(x, y) {
                if transparent_pixel(bitmap, color, state) {
                    continue;
                }
                if bitmap.raw_copy {
                    if matches!(bitmap.mode, 0 | 36) {
                        canvas.store(x, y, color, state);
                    } else {
                        canvas.composite(x, y, color, alpha, bitmap.mode, state);
                    }
                } else {
                    canvas.apply(x, y, color, bitmap.transfer_mode(), state);
                }
            }
        }
    }
    Ok(())
}
