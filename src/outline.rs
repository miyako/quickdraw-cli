use crate::font::TextState;
use crate::model::*;
use crate::raster::TextMask;
use rustybuzz::{
    Direction, Feature, UnicodeBuffer,
    ttf_parser::{self, GlyphId, OutlineBuilder, Tag},
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutlineEncoding {
    Macroman,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutlineSelection {
    pub face_index: u32,
    pub encoding: OutlineEncoding,
    #[serde(default)]
    pub kerning: bool,
}

/// Explicit static TrueType outline data. No platform fonts or legacy mapping inference.
#[derive(Clone, Debug)]
pub struct OutlineFont {
    data: Arc<[u8]>,
    selection: OutlineSelection,
    style: u8,
    complexity: usize,
}

fn malformed(message: &str) -> Error {
    Error::new(ErrorCode::Malformed, message)
}
fn unsupported(message: &str) -> Error {
    Error::new(ErrorCode::Unsupported, message)
}
fn limit(message: &str) -> Error {
    Error::new(ErrorCode::ResourceLimit, message)
}
fn word(data: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_be_bytes(
        data.get(at..at + 2)
            .ok_or_else(|| malformed("truncated outline font"))?
            .try_into()
            .unwrap(),
    ))
}
fn long(data: &[u8], at: usize) -> Result<usize> {
    Ok(u32::from_be_bytes(
        data.get(at..at + 4)
            .ok_or_else(|| malformed("truncated outline font"))?
            .try_into()
            .unwrap(),
    ) as usize)
}

impl OutlineFont {
    pub fn parse(data: &[u8], selection: OutlineSelection, limits: &Limits) -> Result<Self> {
        limits.validate()?;
        if data.len() > limits.max_input_bytes || data.len() > limits.max_decoded_bytes {
            return Err(limit("outline font byte limit exceeded"));
        }
        let mut budget = Budget::new(limits);
        budget.work(data.len() as u64)?;
        let start = if data.starts_with(b"ttcf") {
            let count = long(data, 8)?;
            if selection.face_index as usize >= count {
                return Err(Error::new(
                    ErrorCode::InvalidArguments,
                    "outline collection face index is out of range",
                ));
            }
            long(data, 12 + selection.face_index as usize * 4)?
        } else {
            if selection.face_index != 0 {
                return Err(Error::new(
                    ErrorCode::InvalidArguments,
                    "standalone sfnt face index must be zero",
                ));
            }
            0
        };
        if data.get(start..start + 4) != Some(&[0, 1, 0, 0]) {
            return Err(unsupported(
                "portable outline profile requires static TrueType sfnt/TTC glyf outlines",
            ));
        }
        let count = usize::from(word(data, start + 4)?);
        if count > limits.max_operations {
            return Err(limit("outline table count limit exceeded"));
        }
        let mut tags = std::collections::BTreeSet::new();
        for i in 0..count {
            let at = start + 12 + i * 16;
            let tag = long(data, at)?;
            let offset = long(data, at + 8)?;
            let len = long(data, at + 12)?;
            if !tags.insert(tag) || offset.checked_add(len).is_none_or(|end| end > data.len()) {
                return Err(malformed("duplicate or out-of-bounds sfnt table"));
            }
        }
        let face = ttf_parser::Face::parse(data, selection.face_index)
            .map_err(|_| malformed("invalid outline font face"))?;
        let raw = face.raw_face();
        for tag in [
            b"head", b"hhea", b"hmtx", b"maxp", b"cmap", b"loca", b"glyf", b"OS/2",
        ] {
            if raw.table(Tag::from_bytes(tag)).is_none() {
                return Err(malformed("required outline font table is missing"));
            }
            if face.tables().os2.is_none() {
                return Err(malformed("invalid outline style/metrics table"));
            }
        }
        for tag in [
            b"fvar", b"CFF ", b"CFF2", b"COLR", b"SVG ", b"sbix", b"CBDT", b"EBDT", b"morx",
            b"mort", b"kerx", b"trak",
        ] {
            if raw.table(Tag::from_bytes(tag)).is_some() {
                return Err(unsupported(
                    "variable, color, bitmap-only or AAT outline layout is outside the portable profile",
                ));
            }
        }
        if face.ascender() <= face.descender() || face.units_per_em() == 0 {
            return Err(malformed("invalid outline font vertical metrics"));
        }
        let head = raw.table(Tag::from_bytes(b"head")).unwrap();
        let loca = raw.table(Tag::from_bytes(b"loca")).unwrap();
        let glyf = raw.table(Tag::from_bytes(b"glyf")).unwrap();
        let format = word(head, 50)?;
        if format > 1 {
            return Err(malformed("invalid TrueType loca format"));
        }
        let glyphs = usize::from(face.number_of_glyphs());
        if glyphs > limits.max_operations {
            return Err(limit("outline glyph count limit exceeded"));
        }
        let mut previous = 0;
        budget.allocate((glyphs + 1) * std::mem::size_of::<usize>())?;
        let mut offsets = Vec::with_capacity(glyphs + 1);
        for i in 0..=glyphs {
            let offset = if format == 0 {
                usize::from(word(loca, i * 2)?) * 2
            } else {
                long(loca, i * 4)?
            };
            if offset < previous
                || offset > glyf.len()
                || (offset != previous && offset - previous < 10)
            {
                return Err(malformed("invalid TrueType glyph extent"));
            }
            previous = offset;
            offsets.push(offset);
        }
        let complexity = validate_glyph_graph(glyf, &offsets, &mut budget)?;
        for tag in [b"GSUB", b"GPOS", b"GDEF", b"kern"] {
            let exists = raw.table(Tag::from_bytes(tag)).is_some();
            let parsed = match tag {
                b"GSUB" => face.tables().gsub.is_some(),
                b"GPOS" => face.tables().gpos.is_some(),
                b"GDEF" => face.tables().gdef.is_some(),
                _ => face.tables().kern.is_some(),
            };
            if exists && !parsed {
                return Err(malformed("invalid outline shaping table"));
            }
        }
        let style = u8::from(face.is_bold()) | (u8::from(face.is_italic()) << 1);
        Ok(Self {
            data: data.into(),
            selection,
            style,
            complexity,
        })
    }
    pub(crate) fn style(&self) -> u8 {
        self.style
    }
    pub(crate) fn byte_len(&self) -> usize {
        self.data.len()
    }

    pub(crate) fn layout(
        &self,
        bytes: &[u8],
        baseline: Point,
        text: &TextState,
        budget: &mut Budget<'_>,
    ) -> Result<OutlineRun> {
        if bytes.len() > budget.limits.max_operations {
            return Err(limit("outline input count exceeded"));
        }
        if self.complexity > budget.limits.max_operations {
            return Err(limit(
                "outline expanded glyph complexity exceeds active render limit",
            ));
        }
        budget.work(
            (self.complexity as u64)
                .checked_mul(bytes.len() as u64)
                .ok_or_else(|| limit("outline expansion work overflow"))?,
        )?;
        // Shaper-owned allocations/work cannot be interrupted. Bound the Roman run and
        // conservatively charge font-by-run work before entering the library.
        budget.work(
            (self.data.len() as u64)
                .checked_mul(bytes.len().max(1) as u64)
                .ok_or_else(|| limit("outline shaping work overflow"))?,
        )?;
        budget.allocate(
            bytes
                .len()
                .checked_mul(4096)
                .ok_or_else(|| limit("outline shaping allocation overflow"))?,
        )?;
        let face = rustybuzz::Face::from_slice(&self.data, self.selection.face_index)
            .ok_or_else(|| malformed("invalid outline face"))?;
        let mut buffer = UnicodeBuffer::new();
        buffer.set_direction(Direction::LeftToRight);
        buffer.set_script(rustybuzz::script::LATIN);
        buffer.set_language("und".parse().expect("fixed language"));
        for (i, &byte) in bytes.iter().enumerate() {
            let ch = macroman(byte)?;
            if face.glyph_index(ch).is_none_or(|g| g.0 == 0) {
                return Err(unsupported(&format!(
                    "outline font has no glyph for MacRoman byte 0x{byte:02x} (U+{:04X}); substitution disabled",
                    ch as u32
                )));
            }
            buffer.add(ch, i as u32);
        }
        let features = [
            Feature::new(
                Tag::from_bytes(b"kern"),
                u32::from(self.selection.kerning),
                ..,
            ),
            Feature::new(Tag::from_bytes(b"liga"), 0, ..),
            Feature::new(Tag::from_bytes(b"clig"), 0, ..),
            Feature::new(Tag::from_bytes(b"calt"), 0, ..),
            Feature::new(Tag::from_bytes(b"dlig"), 0, ..),
        ];
        let shaped = rustybuzz::shape(&face, &features, buffer);
        if shaped.len() > budget.limits.max_operations || (!bytes.is_empty() && shaped.is_empty()) {
            return Err(limit("outline shaped glyph count invalid or exceeded"));
        }
        let unit = |v: i64, horizontal| -> Result<i64> {
            let fixed = v
                .checked_mul(i64::from(text.size) * 65536)
                .ok_or_else(|| limit("outline scaling overflow"))?
                .div_euclid(i64::from(face.units_per_em()));
            text.scale(fixed, horizontal)
        };
        let mut x = i64::from(baseline.x) * 65536 + i64::from(text.fraction);
        let y = i64::from(baseline.y) * 65536;
        let mut bounds = Rect {
            left: baseline.x,
            right: baseline.x,
            top: pixel(y - unit(i64::from(face.ascender()), false)?, false)?,
            bottom: pixel(y - unit(i64::from(face.descender()), false)?, true)?,
        };
        let mut glyphs = Vec::new();
        let infos = shaped.glyph_infos();
        if infos.first().is_some_and(|i| i.cluster != 0) {
            return Err(unsupported("outline shaping omitted input cluster"));
        }
        for (i, (info, pos)) in infos.iter().zip(shaped.glyph_positions()).enumerate() {
            if info.glyph_id == 0 || info.glyph_id >= u32::from(face.number_of_glyphs()) {
                return Err(unsupported("outline shaping produced a missing glyph"));
            }
            let cluster = info.cluster as usize;
            let next = infos.get(i + 1).map_or(bytes.len(), |n| n.cluster as usize);
            if cluster >= bytes.len() || next < cluster || next > bytes.len() || pos.y_advance != 0
            {
                return Err(unsupported(
                    "outline profile requires monotone horizontal Roman clusters",
                ));
            }
            let ox = x
                .checked_add(unit(i64::from(pos.x_offset), true)?)
                .ok_or_else(|| limit("outline coordinate overflow"))?;
            let oy = y
                .checked_sub(unit(i64::from(pos.y_offset), false)?)
                .ok_or_else(|| limit("outline coordinate overflow"))?;
            let scale = [
                unit(65536, true)? as f64 / 65536.0 / 65536.0,
                unit(65536, false)? as f64 / 65536.0 / 65536.0,
            ];
            let mut builder = Builder {
                segments: Vec::new(),
                budget,
                error: None,
                scale,
                origin: [ox as f64 / 65536.0, oy as f64 / 65536.0],
            };
            let gid = GlyphId(info.glyph_id as u16);
            let outlined = face.outline_glyph(gid, &mut builder);
            if let Some(error) = builder.error {
                return Err(error);
            }
            if outlined.is_none() && face.glyph_bounding_box(gid).is_some() {
                return Err(malformed("outline glyph could not be decoded"));
            }
            // Empty spaces are valid, failed nonempty glyph decoding is not.
            if outlined.is_none() && !empty_glyph(&face, gid)? {
                return Err(malformed("invalid or unsupported nonempty TrueType glyph"));
            }
            let path = builder.segments;
            for segment in &path {
                for &[px, py] in segment.points() {
                    bounds.left = bounds.left.min(edge(px, false)?);
                    bounds.right = bounds.right.max(edge(px, true)?);
                    bounds.top = bounds.top.min(edge(py, false)?);
                    bounds.bottom = bounds.bottom.max(edge(py, true)?);
                }
            }
            budget.allocate(std::mem::size_of::<Vec<OutlineSegment>>())?;
            glyphs.push(path);
            x = x
                .checked_add(unit(i64::from(pos.x_advance), true)?)
                .ok_or_else(|| limit("outline advance overflow"))?;
            if next != cluster {
                for &byte in &bytes[cluster..next] {
                    x = x
                        .checked_add(text.scale(text.spacing(byte), true)?)
                        .ok_or_else(|| limit("outline spacing overflow"))?;
                }
            }
            bounds.left = bounds.left.min(pixel(x, false)?);
            bounds.right = bounds.right.max(pixel(x, true)?);
        }
        if bounds.width() != 0 && bounds.height() != 0 {
            budget.limits.pixels(bounds.width(), bounds.height())?;
        }
        Ok(OutlineRun {
            glyphs,
            bounds,
            advance_right: pixel(x, false)?,
        })
    }
}

fn empty_glyph(face: &ttf_parser::Face<'_>, gid: GlyphId) -> Result<bool> {
    let raw = face.raw_face();
    let loca = raw.table(Tag::from_bytes(b"loca")).unwrap();
    let head = raw.table(Tag::from_bytes(b"head")).unwrap();
    let i = usize::from(gid.0);
    let (start, end) = if word(head, 50)? == 0 {
        (
            usize::from(word(loca, i * 2)?) * 2,
            usize::from(word(loca, (i + 1) * 2)?) * 2,
        )
    } else {
        (long(loca, i * 4)?, long(loca, (i + 1) * 4)?)
    };
    Ok(start == end || word(raw.table(Tag::from_bytes(b"glyf")).unwrap(), start)? == 0)
}

fn validate_glyph_graph(data: &[u8], offsets: &[usize], budget: &mut Budget<'_>) -> Result<usize> {
    fn visit(
        index: usize,
        data: &[u8],
        offsets: &[usize],
        state: &mut [u8],
        costs: &mut [(usize, usize)],
        nesting: usize,
        budget: &mut Budget<'_>,
    ) -> Result<(usize, usize)> {
        if nesting > 16 {
            return Err(limit("outline composite depth exceeded"));
        }
        if state[index] == 1 {
            return Err(malformed("cyclic outline composite glyph"));
        }
        if state[index] == 2 {
            return Ok(costs[index]);
        }
        state[index] = 1;
        let glyph = &data[offsets[index]..offsets[index + 1]];
        let mut cost = 1usize;
        let mut depth = 0usize;
        if !glyph.is_empty() {
            let contours = word(glyph, 0)? as i16;
            if contours >= 0 {
                let contours = contours as usize;
                let mut previous = None;
                for i in 0..contours {
                    let end = word(glyph, 10 + i * 2)?;
                    if previous.is_some_and(|p| p >= end) {
                        return Err(malformed("invalid outline contour endpoints"));
                    }
                    previous = Some(end);
                }
                let instructions = usize::from(word(glyph, 10 + contours * 2)?);
                if 12 + contours * 2 + instructions > glyph.len() {
                    return Err(malformed("truncated outline instructions"));
                }
                cost += previous.map_or(0, |n| usize::from(n) + 1) + contours * 2;
            } else if contours == -1 {
                let mut at = 10;
                loop {
                    budget.work(1)?;
                    let flags = word(glyph, at)?;
                    let child = usize::from(word(glyph, at + 2)?);
                    if child >= state.len() {
                        return Err(malformed("outline component glyph index out of range"));
                    }
                    if (flags & 8 != 0) as u8 + (flags & 64 != 0) as u8 + (flags & 128 != 0) as u8
                        > 1
                    {
                        return Err(malformed("conflicting outline component transforms"));
                    }
                    at += 4 + if flags & 1 != 0 { 4 } else { 2 };
                    at += if flags & 8 != 0 {
                        2
                    } else if flags & 64 != 0 {
                        4
                    } else if flags & 128 != 0 {
                        8
                    } else {
                        0
                    };
                    if at > glyph.len() {
                        return Err(malformed("truncated outline component"));
                    }
                    let (child_cost, child_depth) =
                        visit(child, data, offsets, state, costs, nesting + 1, budget)?;
                    cost = cost
                        .checked_add(child_cost)
                        .ok_or_else(|| limit("outline composite expansion overflow"))?;
                    depth = depth.max(child_depth + 1);
                    if cost > budget.limits.max_operations || depth > 16 {
                        return Err(limit("outline composite expansion/depth exceeded"));
                    }
                    if flags & 32 == 0 {
                        if flags & 256 != 0 {
                            let n = usize::from(word(glyph, at)?);
                            if at + 2 + n > glyph.len() {
                                return Err(malformed("truncated composite instructions"));
                            }
                        }
                        break;
                    }
                }
            } else {
                return Err(malformed("invalid outline contour count"));
            }
        }
        if cost > budget.limits.max_operations {
            return Err(limit("outline expanded glyph complexity exceeded"));
        }
        budget.work(cost as u64)?;
        state[index] = 2;
        costs[index] = (cost, depth);
        Ok((cost, depth))
    }
    let count = offsets.len() - 1;
    budget.allocate(count * (1 + std::mem::size_of::<(usize, usize)>()))?;
    let mut state = vec![0; count];
    let mut costs = vec![(0, 0); count];
    for index in 0..count {
        visit(index, data, offsets, &mut state, &mut costs, 0, budget)?;
    }
    Ok(costs.iter().map(|&(cost, _)| cost).max().unwrap_or(0))
}
fn pixel(fixed: i64, ceil: bool) -> Result<i32> {
    i32::try_from((fixed + if ceil { 65535 } else { 0 }).div_euclid(65536))
        .map_err(|_| limit("outline coordinate overflow"))
}
fn edge(value: f64, ceil: bool) -> Result<i32> {
    let value = if ceil { value.ceil() } else { value.floor() };
    if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
        return Err(limit("outline coordinate overflow"));
    }
    Ok(value as i32)
}

#[derive(Clone, Copy)]
pub(crate) enum OutlineSegment {
    Move([f64; 2]),
    Line([f64; 2]),
    Quad([[f64; 2]; 2]),
    Curve([[f64; 2]; 3]),
    Close,
}
impl OutlineSegment {
    fn points(&self) -> &[[f64; 2]] {
        match self {
            Self::Move(p) | Self::Line(p) => std::slice::from_ref(p),
            Self::Quad(p) => p,
            Self::Curve(p) => p,
            Self::Close => &[],
        }
    }
}
struct Builder<'a, 'b> {
    segments: Vec<OutlineSegment>,
    budget: &'a mut Budget<'b>,
    error: Option<Error>,
    scale: [f64; 2],
    origin: [f64; 2],
}
impl Builder<'_, '_> {
    fn point(&self, x: f32, y: f32) -> [f64; 2] {
        [
            self.origin[0] + f64::from(x) * self.scale[0],
            self.origin[1] - f64::from(y) * self.scale[1],
        ]
    }
    fn push(&mut self, s: OutlineSegment) {
        if self.error.is_some() {
            return;
        }
        let result = self
            .budget
            .allocate(std::mem::size_of::<OutlineSegment>())
            .and_then(|_| self.budget.work(1));
        if let Err(e) = result {
            self.error = Some(e);
            return;
        }
        if self.segments.len() >= self.budget.limits.max_operations {
            self.error = Some(limit("outline segment count exceeded"));
            return;
        }
        self.segments.push(s);
    }
}
impl OutlineBuilder for Builder<'_, '_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.push(OutlineSegment::Move(self.point(x, y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.push(OutlineSegment::Line(self.point(x, y)));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.push(OutlineSegment::Quad([self.point(x1, y1), self.point(x, y)]));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.push(OutlineSegment::Curve([
            self.point(x1, y1),
            self.point(x2, y2),
            self.point(x, y),
        ]));
    }
    fn close(&mut self) {
        self.push(OutlineSegment::Close);
    }
}

pub(crate) struct OutlineRun {
    pub glyphs: Vec<Vec<OutlineSegment>>,
    pub bounds: Rect,
    pub advance_right: i32,
}
impl OutlineRun {
    pub fn mask(&self, viewport: Rect, masked: bool, budget: &mut Budget<'_>) -> Result<TextMask> {
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
        let count = budget.limits.pixels(bounds.width(), bounds.height())?;
        budget.allocate(count)?;
        budget.work(count as u64)?;
        let mut mask = TextMask {
            bounds,
            ink: vec![0; count],
        };
        for glyph in &self.glyphs {
            let edges = flatten(glyph, budget)?;
            budget.work(
                (count as u64)
                    .checked_mul(edges.len() as u64)
                    .ok_or_else(|| limit("outline coverage work overflow"))?,
            )?;
            for y in bounds.top..bounds.bottom {
                for x in bounds.left..bounds.right {
                    let p = [f64::from(x) + 0.5, f64::from(y) + 0.5];
                    let mut winding = 0i64;
                    for &[a, b] in &edges {
                        let cross = (b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1]);
                        if a[1] <= p[1] && b[1] > p[1] && cross > 0.0 {
                            winding += 1;
                        }
                        if a[1] > p[1] && b[1] <= p[1] && cross < 0.0 {
                            winding -= 1;
                        }
                    }
                    if winding != 0 {
                        mask.ink[(y - bounds.top) as usize * bounds.width()
                            + (x - bounds.left) as usize] = 1;
                    }
                }
            }
        }
        Ok(mask)
    }
}
type Edge = [[f64; 2]; 2];
fn flatten(path: &[OutlineSegment], budget: &mut Budget<'_>) -> Result<Vec<Edge>> {
    fn line(out: &mut Vec<Edge>, a: [f64; 2], b: [f64; 2], budget: &mut Budget<'_>) -> Result<()> {
        budget.allocate(std::mem::size_of::<Edge>())?;
        budget.work(1)?;
        out.push([a, b]);
        Ok(())
    }
    fn curve(
        out: &mut Vec<Edge>,
        p: [[f64; 2]; 4],
        depth: u8,
        budget: &mut Budget<'_>,
    ) -> Result<()> {
        budget.work(1)?;
        let mut error = 0f64;
        for i in 1..3 {
            for (j, _) in p[0].iter().enumerate() {
                error =
                    error.max((p[i][j] - (p[0][j] + (p[3][j] - p[0][j]) * i as f64 / 3.0)).abs());
            }
        }
        if error <= 1.0 / 64.0 {
            return line(out, p[0], p[3], budget);
        }
        if depth == 16 {
            return Err(limit("outline curve subdivision limit exceeded"));
        }
        let mid = |a: [f64; 2], b: [f64; 2]| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        let a = mid(p[0], p[1]);
        let b = mid(p[1], p[2]);
        let c = mid(p[2], p[3]);
        let d = mid(a, b);
        let e = mid(b, c);
        let m = mid(d, e);
        curve(out, [p[0], a, d, m], depth + 1, budget)?;
        curve(out, [m, e, c, p[3]], depth + 1, budget)
    }
    let mut out = Vec::new();
    let mut current = [0.0; 2];
    let mut start = current;
    for &segment in path {
        match segment {
            OutlineSegment::Move(p) => {
                current = p;
                start = p;
            }
            OutlineSegment::Line(p) => {
                line(&mut out, current, p, budget)?;
                current = p;
            }
            OutlineSegment::Quad([control, end]) => {
                let c1 =
                    std::array::from_fn(|j| current[j] + (control[j] - current[j]) * 2.0 / 3.0);
                let c2 = std::array::from_fn(|j| end[j] + (control[j] - end[j]) * 2.0 / 3.0);
                curve(&mut out, [current, c1, c2, end], 0, budget)?;
                current = end;
            }
            OutlineSegment::Curve([a, b, end]) => {
                curve(&mut out, [current, a, b, end], 0, budget)?;
                current = end;
            }
            OutlineSegment::Close => {
                line(&mut out, current, start, budget)?;
                current = start;
            }
        }
    }
    Ok(out)
}

fn macroman(byte: u8) -> Result<char> {
    if byte < 32 || byte == 127 {
        return Err(unsupported(
            "MacRoman control bytes are not supported outline text",
        ));
    }
    const HIGH: [u16; 128] = [
        0x00c4, 0x00c5, 0x00c7, 0x00c9, 0x00d1, 0x00d6, 0x00dc, 0x00e1, 0x00e0, 0x00e2, 0x00e4,
        0x00e3, 0x00e5, 0x00e7, 0x00e9, 0x00e8, 0x00ea, 0x00eb, 0x00ed, 0x00ec, 0x00ee, 0x00ef,
        0x00f1, 0x00f3, 0x00f2, 0x00f4, 0x00f6, 0x00f5, 0x00fa, 0x00f9, 0x00fb, 0x00fc, 0x2020,
        0x00b0, 0x00a2, 0x00a3, 0x00a7, 0x2022, 0x00b6, 0x00df, 0x00ae, 0x00a9, 0x2122, 0x00b4,
        0x00a8, 0x2260, 0x00c6, 0x00d8, 0x221e, 0x00b1, 0x2264, 0x2265, 0x00a5, 0x00b5, 0x2202,
        0x2211, 0x220f, 0x03c0, 0x222b, 0x00aa, 0x00ba, 0x03a9, 0x00e6, 0x00f8, 0x00bf, 0x00a1,
        0x00ac, 0x221a, 0x0192, 0x2248, 0x2206, 0x00ab, 0x00bb, 0x2026, 0x00a0, 0x00c0, 0x00c3,
        0x00d5, 0x0152, 0x0153, 0x2013, 0x2014, 0x201c, 0x201d, 0x2018, 0x2019, 0x00f7, 0x25ca,
        0x00ff, 0x0178, 0x2044, 0x20ac, 0x2039, 0x203a, 0xfb01, 0xfb02, 0x2021, 0x00b7, 0x201a,
        0x201e, 0x2030, 0x00c2, 0x00ca, 0x00c1, 0x00cb, 0x00c8, 0x00cd, 0x00ce, 0x00cf, 0x00cc,
        0x00d3, 0x00d4, 0xf8ff, 0x00d2, 0x00da, 0x00db, 0x00d9, 0x0131, 0x02c6, 0x02dc, 0x00af,
        0x02d8, 0x02d9, 0x02da, 0x00b8, 0x02dd, 0x02db, 0x02c7,
    ];
    Ok(char::from_u32(if byte < 128 {
        u32::from(byte)
    } else {
        u32::from(HIGH[usize::from(byte - 128)])
    })
    .expect("MacRoman scalar"))
}
