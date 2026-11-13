use serde::Serialize;
use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidArguments,
    Io,
    Malformed,
    Unsupported,
    ResourceLimit,
    OutputExists,
    OutputConflict,
    Encoding,
}

#[derive(Debug, Serialize)]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opcode: Option<u16>,
}

impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            offset: None,
            opcode: None,
        }
    }

    pub(crate) fn at(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }

    pub(crate) fn opcode(mut self, opcode: u16) -> Self {
        self.opcode = Some(opcode);
        self
    }

    pub fn exit_code(&self) -> u8 {
        match self.code {
            ErrorCode::InvalidArguments => 2,
            ErrorCode::Io | ErrorCode::OutputExists | ErrorCode::OutputConflict => 3,
            ErrorCode::Malformed => 4,
            ErrorCode::Unsupported => 5,
            ErrorCode::ResourceLimit => 6,
            ErrorCode::Encoding => 7,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(offset) = self.offset {
            write!(f, " at byte {offset}")?;
        }
        Ok(())
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
}

impl Diagnostic {
    pub(crate) fn new(
        code: &'static str,
        message: impl Into<String>,
        offset: Option<usize>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            offset,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, serde::Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Container {
    Auto,
    Raw,
    File,
    #[serde(rename = "resource-fork")]
    ResourceFork,
    #[value(name = "macbinary")]
    #[serde(rename = "macbinary")]
    MacBinary,
    #[value(name = "appledouble")]
    #[serde(rename = "appledouble")]
    AppleDouble,
    #[value(name = "applesingle")]
    #[serde(rename = "applesingle")]
    AppleSingle,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Fidelity {
    Strict,
    Approximate,
}

#[derive(Clone, Debug, Serialize)]
pub struct Limits {
    pub max_input_bytes: usize,
    pub max_pixels: usize,
    pub max_decoded_bytes: usize,
    pub max_operations: usize,
    pub max_work: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_bytes: 64 * 1024 * 1024,
            max_pixels: 16_000_000,
            max_decoded_bytes: 128 * 1024 * 1024,
            max_operations: 100_000,
            max_work: 100_000_000,
        }
    }
}

impl Limits {
    pub fn validate(&self) -> Result<()> {
        if self.max_input_bytes == 0
            || self.max_pixels == 0
            || self.max_decoded_bytes == 0
            || self.max_operations == 0
            || self.max_work == 0
        {
            return Err(Error::new(
                ErrorCode::InvalidArguments,
                "resource limits must be positive",
            ));
        }
        Ok(())
    }

    pub(crate) fn pixels(&self, width: usize, height: usize) -> Result<usize> {
        let pixels = width
            .checked_mul(height)
            .filter(|&n| n <= self.max_pixels)
            .ok_or_else(|| Error::new(ErrorCode::ResourceLimit, "pixel limit exceeded"))?;
        if pixels
            .checked_mul(3)
            .is_none_or(|n| n > self.max_decoded_bytes)
        {
            return Err(Error::new(
                ErrorCode::ResourceLimit,
                "image byte limit exceeded",
            ));
        }
        Ok(pixels)
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct Rect {
    pub top: i32,
    pub left: i32,
    pub bottom: i32,
    pub right: i32,
}

impl Rect {
    pub(crate) fn translated(self, delta: Point) -> Result<Self> {
        let add = |a: i32, b: i32| {
            a.checked_add(b)
                .ok_or_else(|| Error::new(ErrorCode::ResourceLimit, "coordinate range exceeded"))
        };
        Ok(Self {
            top: add(self.top, delta.y)?,
            bottom: add(self.bottom, delta.y)?,
            left: add(self.left, delta.x)?,
            right: add(self.right, delta.x)?,
        })
    }
    pub fn width(self) -> usize {
        (self.right as i64 - self.left as i64).max(0) as usize
    }
    pub fn height(self) -> usize {
        (self.bottom as i64 - self.top as i64).max(0) as usize
    }
    pub(crate) fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
    pub(crate) fn intersect(self, other: Self) -> Self {
        Self {
            top: self.top.max(other.top),
            left: self.left.max(other.left),
            bottom: self.bottom.min(other.bottom),
            right: self.right.min(other.right),
        }
    }
    pub(crate) fn inset(self, x: i32, y: i32) -> Self {
        Self {
            top: self.top + y,
            left: self.left + x,
            bottom: self.bottom - y,
            right: self.right - x,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug)]
pub(crate) struct Region {
    pub bounds: Rect,
    pub mask: Option<Vec<u8>>,
}
impl Region {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.bounds.contains(x, y)
            && self.mask.as_ref().is_none_or(|mask| {
                mask[(y - self.bounds.top) as usize * self.bounds.width()
                    + (x - self.bounds.left) as usize]
                    != 0
            })
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Verb {
    Frame,
    Paint,
    Erase,
    Invert,
    Fill,
}
#[derive(Debug)]
pub(crate) enum Shape {
    Rect(Rect),
    Oval(Rect),
    RoundRect(Rect),
    Arc {
        bounds: Rect,
        start: i32,
        sweep: i32,
    },
    Polygon {
        bounds: Rect,
        points: Vec<Point>,
    },
    Region(Region),
}
#[derive(Debug)]
pub(crate) enum Pixels {
    Mono(Vec<u8>),
    Rgb(Vec<u8>),
}
#[derive(Debug)]
pub(crate) enum Pattern {
    Bits([u8; 8]),
    Solid([u8; 3]),
    Color { bounds: Rect, rgb: Vec<u8> },
}
impl Pattern {
    pub fn sample(&self, x: i32, y: i32) -> [u8; 3] {
        match self {
            Self::Bits(bits) => {
                [if bits[y.rem_euclid(8) as usize] & (0x80 >> x.rem_euclid(8)) != 0 {
                    0
                } else {
                    255
                }; 3]
            }
            Self::Solid(color) => *color,
            Self::Color { bounds, rgb } => {
                let x = x.rem_euclid(bounds.width() as i32) as usize;
                let y = y.rem_euclid(bounds.height() as i32) as usize;
                let i = (y * bounds.width() + x) * 3;
                [rgb[i], rgb[i + 1], rgb[i + 2]]
            }
        }
    }
}
#[derive(Debug)]
pub(crate) struct Bitmap {
    pub bounds: Rect,
    pub source: Rect,
    pub destination: Rect,
    pub mode: u16,
    pub mask: Option<Region>,
    pub pixels: Pixels,
    pub source_mask: Option<Region>,
    pub raw_copy: bool,
    pub affine: Option<Affine>,
    pub alpha: Option<Vec<u8>>,
    pub description_extensions: Option<(usize, Vec<u8>)>,
}

/// QuickTime row-vector affine matrix, retaining 16.16 integers.
#[derive(Debug)]
pub(crate) struct Affine {
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub d: i32,
    pub tx: i32,
    pub ty: i32,
}
impl Affine {
    pub fn determinant(&self) -> i128 {
        i128::from(self.a) * i128::from(self.d) - i128::from(self.b) * i128::from(self.c)
    }
    pub fn bounds(&self, source: Rect) -> Result<Rect> {
        let mut xs = [0i64; 4];
        let mut ys = [0i64; 4];
        for (i, (x, y)) in [
            (source.left, source.top),
            (source.right, source.top),
            (source.left, source.bottom),
            (source.right, source.bottom),
        ]
        .into_iter()
        .enumerate()
        {
            xs[i] = i64::from(x) * i64::from(self.a)
                + i64::from(y) * i64::from(self.c)
                + i64::from(self.tx);
            ys[i] = i64::from(x) * i64::from(self.b)
                + i64::from(y) * i64::from(self.d)
                + i64::from(self.ty);
        }
        let edge = |values: [i64; 4], ceil: bool| -> Result<i32> {
            let v = if ceil {
                *values.iter().max().unwrap() + 65535
            } else {
                *values.iter().min().unwrap()
            };
            i32::try_from(v.div_euclid(65536)).map_err(|_| {
                Error::new(
                    ErrorCode::ResourceLimit,
                    "QuickTime transformed bounds exceed coordinates",
                )
            })
        };
        Ok(Rect {
            left: edge(xs, false)?,
            right: edge(xs, true)?,
            top: edge(ys, false)?,
            bottom: edge(ys, true)?,
        })
    }
    fn source_pixel(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        let x = i128::from(x) * 65536 + 32768 - i128::from(self.tx);
        let y = i128::from(y) * 65536 + 32768 - i128::from(self.ty);
        let determinant = self.determinant();
        let floor = |n: i128| {
            let (n, d) = if determinant < 0 {
                (-n, -determinant)
            } else {
                (n, determinant)
            };
            i32::try_from(n.div_euclid(d)).ok()
        };
        Some((
            floor(x * i128::from(self.d) - y * i128::from(self.c))?,
            floor(y * i128::from(self.a) - x * i128::from(self.b))?,
        ))
    }
}
impl Bitmap {
    pub fn transfer_mode(&self) -> u16 {
        // The parser admits bit 64 only without source precision or spatial reduction.
        if !self.raw_copy && (64..=71).contains(&self.mode) {
            self.mode - 64
        } else {
            self.mode
        }
    }
    pub fn sample(&self, x: i32, y: i32) -> Option<([u8; 3], u8)> {
        let dest = self.destination;
        if !dest.contains(x, y) || self.mask.as_ref().is_some_and(|m| !m.contains(x, y)) {
            return None;
        }
        let (sx, sy) = if let Some(transform) = &self.affine {
            transform.source_pixel(x, y)?
        } else {
            let sx = self.source.left
                + (((x as i64 - dest.left as i64) * self.source.width() as i64)
                    / dest.width() as i64) as i32;
            let sy = self.source.top
                + (((y as i64 - dest.top as i64) * self.source.height() as i64)
                    / dest.height() as i64) as i32;
            (sx, sy)
        };
        if !self.source.contains(sx, sy) {
            return None;
        }
        if self
            .source_mask
            .as_ref()
            .is_some_and(|m| !m.contains(sx, sy))
        {
            return None;
        }
        let index = (sy - self.bounds.top) as usize * self.bounds.width()
            + (sx - self.bounds.left) as usize;
        let rgb = match &self.pixels {
            Pixels::Mono(p) => [if p[index] == 0 { 255 } else { 0 }; 3],
            Pixels::Rgb(p) => [p[index * 3], p[index * 3 + 1], p[index * 3 + 2]],
        };
        Some((rgb, self.alpha.as_ref().map_or(255, |a| a[index])))
    }
}

#[derive(Debug)]
pub(crate) enum Command {
    Nop,
    Clip(Region),
    BackgroundPattern(Pattern),
    PenPattern(Pattern),
    FillPattern(Pattern),
    Foreground([u8; 3]),
    Background([u8; 3]),
    PenSize(Point),
    PenMode(u16),
    Origin(Point),
    OvalSize(Point),
    OpColor([u16; 3]),
    HighlightColor(Option<[u8; 3]>),
    Highlight,
    Line(Point, Point),
    LineFrom(Point),
    ShortLineFrom(Point),
    TextState {
        opcode: u16,
        data: Vec<u8>,
    },
    Text {
        position: Option<Point>,
        delta: Point,
        bytes: Vec<u8>,
    },
    Draw(Verb, Shape),
    Bitmap(Bitmap),
    Comment {
        kind: u16,
        data: Vec<u8>,
    },
    Metadata(Vec<u8>),
    Reserved(Vec<u8>),
    Unsupported {
        feature: &'static str,
        data: Vec<u8>,
    },
}
#[derive(Debug)]
pub(crate) struct Operation {
    pub offset: usize,
    pub opcode: u16,
    pub command: Command,
}

#[derive(Debug)]
pub struct Picture {
    pub(crate) resource_id: Option<i16>,
    pub(crate) frame: Rect,
    pub(crate) version: u8,
    pub(crate) container: Container,
    pub(crate) source_offset: usize,
    pub(crate) consumed_bytes: usize,
    pub(crate) resolution: Option<[f64; 2]>,
    pub(crate) header: Vec<u8>,
    pub(crate) operations: Vec<Operation>,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

#[derive(Serialize)]
pub struct InspectedOperation {
    pub offset: usize,
    pub opcode: u16,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment_kind: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_description_extensions: Option<InspectedDescriptionExtensions>,
}

#[derive(Serialize)]
pub struct InspectedDescriptionExtensions {
    pub offset: usize,
    pub data_hex: String,
}

#[derive(Serialize)]
pub struct Inspection {
    pub resource_id: Option<i16>,
    pub pict_version: u8,
    pub container: Container,
    pub source_offset: usize,
    pub consumed_bytes: usize,
    pub frame: Rect,
    pub resolution_dpi: Option<[f64; 2]>,
    pub header_hex: String,
    pub operations: Vec<InspectedOperation>,
    pub diagnostics: Vec<Diagnostic>,
}

fn hex(data: &[u8]) -> String {
    use std::fmt::Write;
    let mut text = String::with_capacity(data.len() * 2);
    for byte in data {
        write!(&mut text, "{byte:02x}").expect("writing to String");
    }
    text
}

impl Picture {
    pub fn resource_id(&self) -> Option<i16> {
        self.resource_id
    }
    pub fn frame(&self) -> Rect {
        self.frame
    }

    /// Inspect the parsed stream. Successful inspection does not imply renderability.
    pub fn inspect(&self) -> Inspection {
        Inspection {
            resource_id: self.resource_id,
            pict_version: self.version,
            container: self.container,
            source_offset: self.source_offset,
            consumed_bytes: self.consumed_bytes,
            frame: self.frame,
            resolution_dpi: self.resolution,
            header_hex: hex(&self.header),
            diagnostics: self.diagnostics.clone(),
            operations: self
                .operations
                .iter()
                .map(|op| {
                    let mut info = InspectedOperation {
                        offset: op.offset,
                        opcode: op.opcode,
                        kind: "state",
                        comment_kind: None,
                        data_hex: None,
                        image_description_extensions: None,
                    };
                    match &op.command {
                        Command::Comment { kind, data } => {
                            info.kind = "comment";
                            info.comment_kind = Some(*kind);
                            info.data_hex = Some(hex(data));
                        }
                        Command::Unsupported { feature, data } => {
                            info.kind = feature;
                            info.data_hex = Some(hex(data));
                        }
                        Command::Reserved(data) => {
                            info.kind = "reserved";
                            info.data_hex = Some(hex(data));
                        }
                        Command::Text { bytes: data, .. } => {
                            info.kind = "text";
                            info.data_hex = Some(hex(data));
                        }
                        Command::TextState { data, .. } | Command::Metadata(data) => {
                            info.kind = "metadata";
                            info.data_hex = Some(hex(data));
                        }
                        Command::Draw(_, _) => info.kind = "shape",
                        Command::Bitmap(bitmap) => {
                            info.kind = "bitmap";
                            info.image_description_extensions = bitmap
                                .description_extensions
                                .as_ref()
                                .map(|(offset, data)| InspectedDescriptionExtensions {
                                    offset: *offset,
                                    data_hex: hex(data),
                                });
                        }
                        Command::Line(_, _) | Command::LineFrom(_) | Command::ShortLineFrom(_) => {
                            info.kind = "line"
                        }
                        _ => {}
                    }
                    info
                })
                .collect(),
        }
    }
}

pub(crate) struct Budget<'a> {
    pub limits: &'a Limits,
    decoded: usize,
    work: u64,
}
impl<'a> Budget<'a> {
    pub fn new(limits: &'a Limits) -> Self {
        Self {
            limits,
            decoded: 0,
            work: 0,
        }
    }
    pub fn allocate(&mut self, bytes: usize) -> Result<()> {
        self.decoded = self
            .decoded
            .checked_add(bytes)
            .filter(|&n| n <= self.limits.max_decoded_bytes)
            .ok_or_else(|| Error::new(ErrorCode::ResourceLimit, "decoded byte budget exceeded"))?;
        Ok(())
    }
    pub fn work(&mut self, count: u64) -> Result<()> {
        self.work = self
            .work
            .checked_add(count)
            .filter(|&n| n <= self.limits.max_work)
            .ok_or_else(|| Error::new(ErrorCode::ResourceLimit, "work budget exceeded"))?;
        Ok(())
    }
}
