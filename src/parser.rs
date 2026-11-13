use crate::model::*;

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    base: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], base: usize) -> Self {
        Self {
            bytes,
            pos: 0,
            base,
        }
    }
    fn offset(&self) -> usize {
        self.base + self.pos
    }
    fn take(&mut self, size: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(size)
            .filter(|&end| end <= self.bytes.len())
            .ok_or_else(|| {
                Error::new(ErrorCode::Malformed, "truncated PICT data").at(self.offset())
            })?;
        let data = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(data)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn i16(&mut self) -> Result<i32> {
        Ok(self.u16()? as i16 as i32)
    }
    fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn point(&mut self) -> Result<Point> {
        let y = self.i16()?;
        Ok(Point { x: self.i16()?, y })
    }
    fn rect(&mut self) -> Result<Rect> {
        Ok(Rect {
            top: self.i16()?,
            left: self.i16()?,
            bottom: self.i16()?,
            right: self.i16()?,
        })
    }
    fn color(&mut self) -> Result<[u8; 3]> {
        Ok([
            (self.u16()? >> 8) as u8,
            (self.u16()? >> 8) as u8,
            (self.u16()? >> 8) as u8,
        ])
    }
    fn pattern(&mut self) -> Result<[u8; 8]> {
        let mut p = [0; 8];
        p.copy_from_slice(self.take(8)?);
        Ok(p)
    }
    fn blob(&mut self, size: usize, budget: &mut Budget<'_>) -> Result<Vec<u8>> {
        let b = self.take(size)?;
        budget.allocate(size)?;
        Ok(b.to_vec())
    }
}

fn malformed(message: &str) -> Error {
    Error::new(ErrorCode::Malformed, message)
}
fn unsupported(message: &str) -> Error {
    Error::new(ErrorCode::Unsupported, message)
}

pub(crate) fn signature(data: &[u8], start: usize) -> bool {
    data.get(start + 10..start + 12) == Some(&[0x11, 1])
        || data.get(start + 10..start + 14) == Some(&[0, 0x11, 2, 0xff])
}

/// Parse an unambiguously selected picture. Use `parse_selected` for multi-picture containers.
pub fn parse(data: &[u8], container: Container, limits: &Limits) -> Result<Picture> {
    crate::parse_selected(data, container, crate::PictureSelector::Unique, limits)
}

pub(crate) fn parse_payload(
    bytes: &[u8],
    start: usize,
    container: Container,
    limits: &Limits,
) -> Result<Picture> {
    let mut r = Reader::new(bytes, start);
    let declared_size = r.u16()? as usize;
    let frame = r.rect()?;
    if frame.width() == 0 || frame.height() == 0 {
        return Err(malformed("picture frame must have positive dimensions").at(start + 2));
    }
    limits.pixels(frame.width(), frame.height())?;
    let version = match r.u16()? {
        0x1101 => 1,
        0x0011 if r.u16()? == 0x02ff => 2,
        _ => return Err(malformed("invalid PICT version signature").at(start + 10)),
    };
    let mut budget = Budget::new(limits);
    budget.work(bytes.len() as u64)?;
    let mut diagnostics = Vec::new();
    let mut resolution = None;
    let mut header = Vec::new();
    if version == 2 {
        if r.u16()? != 0x0c00 {
            return Err(malformed("PICT v2 must start with HeaderOp").at(r.offset() - 2));
        }
        let header_start = r.offset();
        let h = r.take(24)?;
        header = h.to_vec();
        let mut hr = Reader::new(h, header_start);
        match hr.u16()? {
            0xffff => {
                if hr.u16()? != 0xffff {
                    return Err(malformed("invalid original v2 header").at(header_start));
                }
            }
            0xfffe => {
                hr.u16()?;
                let hres = hr.u32()? as i32;
                let vres = hr.u32()? as i32;
                if hres <= 0 || vres <= 0 {
                    return Err(malformed("nonpositive picture resolution").at(header_start));
                }
                resolution = Some([hres as f64 / 65536.0, vres as f64 / 65536.0]);
                let source = hr.rect()?;
                if source != frame {
                    return Err(unsupported(
                        "extended header source rectangle differs from picFrame",
                    )
                    .at(header_start));
                }
            }
            _ => return Err(unsupported("unknown v2 header version").at(header_start)),
        }
    }
    let mut operations = Vec::new();
    let mut last_rect = None;
    loop {
        if version == 2 && r.pos % 2 == 1 {
            let pad = r.u8()?;
            if pad != 0 {
                return Err(malformed("nonzero word-alignment padding").at(r.offset() - 1));
            }
        }
        let offset = r.offset();
        let opcode = if version == 1 {
            r.u8()? as u16
        } else {
            r.u16()?
        };
        if opcode == 0xff {
            break;
        }
        if operations.len() >= limits.max_operations {
            return Err(
                Error::new(ErrorCode::ResourceLimit, "operation limit exceeded").at(offset),
            );
        }
        budget.allocate(std::mem::size_of::<Operation>())?;
        budget.work(1)?;
        let result = command(&mut r, opcode, version, &mut budget, &mut last_rect);
        let command = result.map_err(|mut e| {
            if e.offset.is_none() {
                e.offset = Some(offset);
            }
            e.opcode(opcode)
        })?;
        if let Command::Unsupported { feature, .. } = &command {
            diagnostics.push(Diagnostic::new(
                "unsupported_operation",
                *feature,
                Some(offset),
            ));
        }
        operations.push(Operation {
            offset,
            opcode,
            command,
        });
    }
    if version == 1 && declared_size != 0 && declared_size != r.pos {
        return Err(malformed("v1 picSize does not match the end-of-picture offset").at(start));
    }
    if r.pos < bytes.len() {
        return Err(
            malformed("trailing bytes after EndPic; extract one PICT resource first")
                .at(r.offset()),
        );
    }
    Ok(Picture {
        resource_id: None,
        frame,
        version,
        container,
        source_offset: start,
        consumed_bytes: r.pos,
        resolution,
        header,
        operations,
        diagnostics,
    })
}

fn classic_color(value: u32) -> Result<[u8; 3]> {
    match value {
        30 => Ok([255, 255, 255]),
        33 => Ok([0, 0, 0]),
        69 => Ok([255, 255, 0]),
        137 => Ok([255, 0, 255]),
        205 => Ok([255, 0, 0]),
        273 => Ok([0, 255, 255]),
        341 => Ok([0, 255, 0]),
        409 => Ok([0, 0, 255]),
        _ => Err(unsupported("unknown classic color constant")),
    }
}

fn verb(op: u16) -> Verb {
    match op & 7 {
        0 => Verb::Frame,
        1 => Verb::Paint,
        2 => Verb::Erase,
        3 => Verb::Invert,
        _ => Verb::Fill,
    }
}

fn command(
    r: &mut Reader<'_>,
    op: u16,
    version: u8,
    budget: &mut Budget<'_>,
    last_rect: &mut Option<Rect>,
) -> Result<Command> {
    if version == 1
        && !matches!(
            op,
            0x00..=0x10 | 0x20..=0x23 | 0x28..=0x2b
                | 0x30..=0x34 | 0x38..=0x3c | 0x40..=0x44 | 0x48..=0x4c
                | 0x50..=0x54 | 0x58..=0x5c | 0x60..=0x64 | 0x68..=0x6c
                | 0x70..=0x74 | 0x78..=0x7c | 0x80..=0x84 | 0x88..=0x8c
                | 0x90 | 0x91 | 0x98 | 0x99 | 0xa0 | 0xa1
        )
    {
        return Err(unsupported("opcode is not in the supported PICT v1 table"));
    }
    Ok(match op {
        0 => Command::Nop,
        1 => Command::Clip(region(r, budget)?),
        2 => Command::BackgroundPattern(Pattern::Bits(r.pattern()?)),
        7 => {
            let p = r.point()?;
            if p.x < 0 || p.y < 0 {
                return Err(malformed("negative pen size"));
            }
            Command::PenSize(p)
        }
        8 => Command::PenMode(r.u16()?),
        9 => Command::PenPattern(Pattern::Bits(r.pattern()?)),
        10 => Command::FillPattern(Pattern::Bits(r.pattern()?)),
        0x0b => {
            let size = r.point()?;
            if size.x < 0 || size.y < 0 {
                return Err(malformed("negative oval size"));
            }
            Command::OvalSize(size)
        }
        0x0e => Command::Foreground(classic_color(r.u32()?)?),
        0x0f => Command::Background(classic_color(r.u32()?)?),
        0x1a => Command::Foreground(r.color()?),
        0x1b => Command::Background(r.color()?),
        0x03 | 0x05 | 0x0d | 0x15 | 0x16 => Command::TextState {
            opcode: op,
            data: r.blob(2, budget)?,
        },
        0x04 => Command::TextState {
            opcode: op,
            data: r.blob(1, budget)?,
        },
        0x06 => Command::TextState {
            opcode: op,
            data: r.blob(4, budget)?,
        },
        0x10 => Command::TextState {
            opcode: op,
            data: r.blob(8, budget)?,
        },
        0x1d => Command::HighlightColor(Some(r.color()?)),
        0x1f => Command::OpColor([r.u16()?, r.u16()?, r.u16()?]),
        0x1e => Command::HighlightColor(None),
        0x0c => Command::Origin(Point {
            x: r.i16()?,
            y: r.i16()?,
        }),
        0x1c => Command::Highlight,
        0x21 => Command::LineFrom(r.point()?),
        0x23 => Command::ShortLineFrom(Point {
            x: r.u8()? as i8 as i32,
            y: r.u8()? as i8 as i32,
        }),
        0x20 | 0x22 => {
            let from = r.point()?;
            let to = if op == 0x20 {
                r.point()?
            } else {
                let x = from.x + r.u8()? as i8 as i32;
                let y = from.y + r.u8()? as i8 as i32;
                if i16::try_from(x).is_err() || i16::try_from(y).is_err() {
                    return Err(malformed("short line coordinate overflow"));
                }
                Point { x, y }
            };
            Command::Line(from, to)
        }
        0x28..=0x2b => {
            let position = if op == 0x28 { Some(r.point()?) } else { None };
            let delta = Point {
                x: if op == 0x29 || op == 0x2b {
                    r.u8()? as i32
                } else {
                    0
                },
                y: if op == 0x2a || op == 0x2b {
                    r.u8()? as i32
                } else {
                    0
                },
            };
            let count = r.u8()? as usize;
            Command::Text {
                position,
                delta,
                bytes: r.blob(count, budget)?,
            }
        }
        0x2c..=0x2e => {
            let n = r.u16()? as usize;
            let data = r.blob(n, budget)?;
            if op == 0x2c {
                if n < 3 || data[2] as usize + 3 != n {
                    return Err(malformed("invalid fontName length"));
                }
                Command::Metadata(data)
            } else {
                let known_size = if op == 0x2d { 8 } else { 4 };
                if n < known_size {
                    return Err(malformed("invalid text metadata length"));
                }
                if n > known_size {
                    Command::Unsupported {
                        feature: "extended_text_state",
                        data,
                    }
                } else {
                    Command::TextState { opcode: op, data }
                }
            }
        }
        0x30..=0x6c if op & 7 <= 4 => {
            let rect = if op & 8 == 0 {
                let rect = r.rect()?;
                *last_rect = Some(rect);
                rect
            } else {
                last_rect
                    .ok_or_else(|| malformed("same-shape opcode without a preceding rectangle"))?
            };
            let family = op & 0xf0;
            Command::Draw(
                verb(op),
                match family {
                    0x30 => Shape::Rect(rect),
                    0x40 => Shape::RoundRect(rect),
                    0x50 => Shape::Oval(rect),
                    _ => Shape::Arc {
                        bounds: rect,
                        start: r.i16()?,
                        sweep: r.i16()?,
                    },
                },
            )
        }
        0x70..=0x74 => {
            let size = r.u16()? as usize;
            if size < 10 || (size - 10) % 4 != 0 {
                return Err(malformed("invalid polygon length"));
            }
            let bounds = r.rect()?;
            let count = (size - 10) / 4;
            budget.allocate(count * std::mem::size_of::<Point>())?;
            let mut points = Vec::with_capacity(count);
            for _ in 0..count {
                let p = r.point()?;
                if p.x < bounds.left
                    || p.x > bounds.right
                    || p.y < bounds.top
                    || p.y > bounds.bottom
                {
                    return Err(malformed("polygon point outside declared bounds"));
                }
                points.push(p);
            }
            Command::Draw(verb(op), Shape::Polygon { bounds, points })
        }
        0x80..=0x84 => Command::Draw(verb(op), Shape::Region(region(r, budget)?)),
        0x78..=0x7c | 0x88..=0x8c => Command::Unsupported {
            feature: "same_polygon_or_region",
            data: Vec::new(),
        },
        0x90 | 0x91 | 0x98..=0x9b => Command::Bitmap(bitmap(r, op, version, budget)?),
        0xa0 => Command::Comment {
            kind: r.u16()?,
            data: Vec::new(),
        },
        0xa1 => {
            let kind = r.u16()?;
            let size = r.u16()? as usize;
            Command::Comment {
                kind,
                data: r.blob(size, budget)?,
            }
        }
        0x8200 => quicktime(r, budget)?,
        0x8201 => {
            let size = r.u32()? as usize;
            Command::Unsupported {
                feature: "quicktime_image",
                data: r.blob(size, budget)?,
            }
        }
        0x12..=0x14 => {
            let pattern = pixel_pattern(r, budget)?;
            match op {
                0x12 => Command::BackgroundPattern(pattern),
                0x13 => Command::PenPattern(pattern),
                _ => Command::FillPattern(pattern),
            }
        }
        0x24..=0x27 | 0x2f | 0x92..=0x97 | 0x9c..=0x9f | 0xa2..=0xaf => {
            let size = r.u16()? as usize;
            Command::Reserved(r.blob(size, budget)?)
        }
        0x35..=0x6f => {
            let size = if op & 8 == 0 { 8 } else { 0 } + if op & 0xf0 == 0x60 { 4 } else { 0 };
            Command::Reserved(r.blob(size, budget)?)
        }
        0x75..=0x77 | 0x85..=0x87 => {
            let size = r.u16()? as usize;
            if size < 10 {
                return Err(malformed("invalid reserved geometry length"));
            }
            Command::Reserved(r.blob(size - 2, budget)?)
        }
        0x7d..=0x7f | 0x8d..=0x8f | 0xb0..=0xcf | 0x8000..=0x80ff => Command::Reserved(Vec::new()),
        0xd0..=0xfe | 0x8100..=0xffff => {
            let size = r.u32()? as usize;
            Command::Reserved(r.blob(size, budget)?)
        }
        0x100..=0x7fff if op != 0x0c00 => {
            Command::Reserved(r.blob((op >> 8) as usize * 2, budget)?)
        }
        _ => return Err(unsupported("unknown opcode or unsupported opcode layout")),
    })
}

fn region(r: &mut Reader<'_>, budget: &mut Budget<'_>) -> Result<Region> {
    let offset = r.offset();
    let size = r.u16()? as usize;
    if size < 10 || size % 2 != 0 {
        return Err(malformed("invalid region size"));
    }
    let mut rr = Reader::new(r.take(size - 2)?, offset + 2);
    let bounds = rr.rect()?;
    if bounds.bottom < bounds.top || bounds.right < bounds.left {
        return Err(malformed("inverted region bounds"));
    }
    if size == 10 {
        return Ok(Region { bounds, mask: None });
    }
    let width = bounds.width();
    let height = bounds.height();
    if width == 0 || height == 0 {
        return Err(malformed("complex region must have nonempty bounds"));
    }
    let count = budget.limits.pixels(width, height)?;
    budget.allocate(count + width + (size - 10) * std::mem::size_of::<i32>())?;
    budget.work(count as u64)?;
    let mut mask = vec![0; count];
    let mut active = vec![0u8; width];
    let mut previous = bounds.top;
    let mut seen_row = false;
    loop {
        let y = rr.i16()?;
        if y == 0x7fff {
            break;
        }
        if y < bounds.top || y > bounds.bottom || (seen_row && y <= previous) {
            return Err(malformed("region rows must be ordered and within bounds"));
        }
        for row in previous..y {
            let start = (row - bounds.top) as usize * width;
            mask[start..start + width].copy_from_slice(&active);
        }
        previous = y;
        seen_row = true;
        let mut xs = Vec::new();
        loop {
            let x = rr.i16()?;
            if x == 0x7fff {
                break;
            }
            if x < bounds.left || x > bounds.right || xs.last().is_some_and(|&last| last >= x) {
                return Err(malformed(
                    "region transitions must be ordered and within bounds",
                ));
            }
            xs.push(x);
        }
        if xs.len() % 2 != 0 {
            return Err(malformed("unpaired region transitions"));
        }
        for pair in xs.chunks_exact(2) {
            budget.work((pair[1] - pair[0]) as u64)?;
            for value in
                &mut active[(pair[0] - bounds.left) as usize..(pair[1] - bounds.left) as usize]
            {
                *value ^= 1;
            }
        }
    }
    if rr.pos != rr.bytes.len() || active.iter().any(|&v| v != 0) {
        return Err(malformed(
            "region has trailing data or unclosed transitions",
        ));
    }
    Ok(Region {
        bounds,
        mask: Some(mask),
    })
}

/// PackBits expands signed control bytes; -128 is a no-op. Unit size 2 is used by RGB555.
fn unpack(data: &[u8], expected: usize, unit: usize) -> Result<Vec<u8>> {
    let mut r = Reader::new(data, 0);
    let mut out = Vec::with_capacity(expected);
    while r.pos < data.len() {
        let control = r.u8()? as i8;
        if control == -128 {
            continue;
        }
        let count = if control >= 0 {
            control as usize + 1
        } else {
            (1 - control as i16) as usize
        };
        let bytes = count * unit;
        if out.len().checked_add(bytes).is_none_or(|n| n > expected) {
            return Err(malformed("PackBits expands beyond its scanline"));
        }
        if control >= 0 {
            out.extend_from_slice(r.take(bytes)?);
        } else {
            let value = r.take(unit)?;
            for _ in 0..count {
                out.extend_from_slice(value);
            }
        }
    }
    if out.len() != expected {
        return Err(malformed("PackBits scanline length mismatch"));
    }
    Ok(out)
}

fn bitmap(r: &mut Reader<'_>, op: u16, version: u8, budget: &mut Budget<'_>) -> Result<Bitmap> {
    let direct = op == 0x9a || op == 0x9b;
    if direct {
        if version == 1 {
            return Err(unsupported("direct pixel maps in PICT v1"));
        }
        if r.u32()? != 0xff {
            return Err(malformed("invalid direct PixMap base address sentinel"));
        }
    }
    let layout = map_layout(r, direct, version, budget)?;
    if matches!(op, 0x90 | 0x91) && layout.row_bytes >= 8 {
        return Err(malformed("BitsRect/BitsRgn requires rowBytes less than 8"));
    }
    let bounds = layout.bounds;
    let source = r.rect()?;
    let destination = r.rect()?;
    if source.width() == 0
        || source.height() == 0
        || source.left < bounds.left
        || source.top < bounds.top
        || source.right > bounds.right
        || source.bottom > bounds.bottom
    {
        return Err(malformed("bitmap source rectangle outside bounds or empty"));
    }
    let mode = r.u16()?;
    let mask = if op & 1 != 0 {
        Some(region(r, budget)?)
    } else {
        None
    };
    let pixels = map_pixels(r, layout, op & 8 != 0, budget)?;
    Ok(Bitmap {
        bounds,
        source,
        destination,
        mode,
        mask,
        pixels,
        source_mask: None,
        raw_copy: false,
        affine: None,
        alpha: None,
    })
}

fn quicktime(r: &mut Reader<'_>, budget: &mut Budget<'_>) -> Result<Command> {
    let size = r.u32()? as usize;
    let base = r.offset();
    let data = r.take(size)?;
    let mut q = Reader::new(data, base);
    if q.u16()? != 0 {
        budget.allocate(data.len())?;
        return Ok(Command::Unsupported {
            feature: "quicktime_opcode_version",
            data: data.to_vec(),
        });
    }
    let mut matrix = [0i32; 9];
    for v in &mut matrix {
        *v = q.u32()? as i32;
    }
    if matrix[2] != 0 || matrix[5] != 0 || matrix[8] != 0x40000000 {
        return Err(unsupported(
            "QuickTime perspective/projective matrices are not implemented",
        ));
    }
    let matte_size = q.u32()? as usize;
    let matte = q.rect()?;
    if matte_size != 0 || matte.width() != 0 || matte.height() != 0 {
        return Err(unsupported(
            "QuickTime separate matte payload/layout is not implemented; alpha modes are distinct",
        ));
    }
    let mode = q.u16()?;
    if !matches!(mode, 0 | 256 | 257 | 258) {
        return Err(unsupported(
            "QuickTime still images support srcCopy, straight alpha and white/black premultiplied alpha modes only",
        ));
    }
    let source = q.rect()?;
    q.u32()?; // accuracy is a decompressor preference, not a spatial transform
    let mask_size = q.u32()? as usize;
    let source_mask = if mask_size == 0 {
        None
    } else {
        if mask_size > 65525 {
            return Err(unsupported(
                "QuickTime source mask exceeds supported region size",
            ));
        }
        let mask = q.take(mask_size)?;
        budget.allocate(mask_size + 10)?;
        let mut packed = ((mask_size + 10) as u16).to_be_bytes().to_vec();
        for v in [source.top, source.left, source.bottom, source.right] {
            packed.extend_from_slice(&(v as i16).to_be_bytes());
        }
        packed.extend_from_slice(mask);
        Some(region(
            &mut Reader::new(&packed, base + q.pos - mask_size - 10),
            budget,
        )?)
    };
    let at = q.pos;
    let description_size = q.u32()? as usize;
    if description_size < 86 {
        return Err(malformed("QuickTime image description too short"));
    }
    q.take(description_size - 4)?;
    if description_size != 86 {
        return Err(unsupported(
            "QuickTime image-description extensions are not implemented",
        ));
    }
    let mut d = Reader::new(&data[at..at + 86], base + at);
    d.u32()?;
    let codec = d.take(4)?;
    d.take(8)?;
    if d.u16()? != 0 || d.u16()? != 0 {
        return Err(unsupported(
            "QuickTime image-description version not implemented",
        ));
    }
    d.take(12)?;
    let width = d.u16()? as usize;
    let height = d.u16()? as usize;
    if width == 0 || height == 0 {
        return Err(malformed("empty QuickTime image"));
    }
    budget.limits.pixels(width, height)?;
    d.take(8)?;
    let compressed_size = d.u32()? as usize;
    if d.u16()? != 1 {
        return Err(unsupported("QuickTime still image must have one frame"));
    }
    d.take(32)?;
    let depth = d.u16()?;
    let supported_depth = if matches!(codec, b"raw " | b"rle ") {
        matches!(depth, 16 | 24 | 32)
    } else {
        matches!(depth, 8 | 24 | 32 | 40)
    };
    if !supported_depth || d.u16()? != 0xffff {
        return Err(unsupported(
            "QuickTime image depth or external color table not implemented",
        ));
    }
    if compressed_size != data.len() - q.pos {
        return Err(malformed("QuickTime compressed image length mismatch"));
    }
    let bounds = Rect {
        top: 0,
        left: 0,
        bottom: height as i32,
        right: width as i32,
    };
    if source.width() == 0 || source.height() == 0 || source.intersect(bounds) != source {
        return Err(malformed(
            "QuickTime source rectangle outside decoded image",
        ));
    }
    let transform = Affine {
        a: matrix[0],
        b: matrix[1],
        c: matrix[3],
        d: matrix[4],
        tx: matrix[6],
        ty: matrix[7],
    };
    if transform.determinant() == 0 {
        return Err(malformed("singular QuickTime affine matrix"));
    }
    let destination = transform.bounds(source)?;
    // Preserve the existing top-left sampling contract for integral positive axis placement.
    let legacy = matrix[1] == 0
        && matrix[3] == 0
        && matrix[0] > 0
        && matrix[4] > 0
        && [
            (source.left, matrix[0], matrix[6]),
            (source.right, matrix[0], matrix[6]),
            (source.top, matrix[4], matrix[7]),
            (source.bottom, matrix[4], matrix[7]),
        ]
        .iter()
        .all(|&(v, s, t)| (i64::from(v) * i64::from(s) + i64::from(t)) % 65536 == 0);
    let decoded = crate::codec::decode(
        codec,
        q.take(compressed_size)?,
        width,
        height,
        depth,
        budget,
    )?;
    if mode == 0
        && decoded
            .alpha
            .as_ref()
            .is_some_and(|a| a.iter().any(|&a| a != 255))
    {
        return Err(unsupported(
            "nonopaque QuickTime srcCopy is outside the portable profile; an alpha transfer mode must be recorded",
        ));
    }
    if mode != 0 && decoded.alpha.is_none() {
        return Err(unsupported(
            "QuickTime alpha mode requires an explicit alpha channel",
        ));
    }
    if matches!(mode, 257 | 258) && codec != b"raw " && codec != b"rle " {
        return Err(unsupported(
            "premultiplied QuickTime alpha supports explicit ARGB raw/Animation data only",
        ));
    }
    if matches!(mode, 257 | 258) {
        for (rgb, &alpha) in decoded
            .rgb
            .chunks_exact(3)
            .zip(decoded.alpha.as_ref().expect("alpha checked"))
        {
            if rgb.iter().any(|&v| {
                if mode == 258 {
                    v > alpha
                } else {
                    v < 255 - alpha
                }
            }) {
                return Err(malformed(
                    "QuickTime color samples contradict recorded premultiplied alpha mode",
                ));
            }
        }
    }
    Ok(Command::Bitmap(Bitmap {
        bounds,
        source,
        destination,
        mode,
        mask: None,
        pixels: Pixels::Rgb(decoded.rgb),
        source_mask,
        raw_copy: true,
        affine: (!legacy).then_some(transform),
        alpha: decoded.alpha,
    }))
}

struct MapLayout {
    bounds: Rect,
    row_bytes: usize,
    pixmap: bool,
    direct: bool,
    pack_type: u16,
    depth: u16,
    components: u16,
    palette: [Option<[u8; 3]>; 256],
}

fn map_layout(
    r: &mut Reader<'_>,
    direct: bool,
    version: u8,
    budget: &Budget<'_>,
) -> Result<MapLayout> {
    let row_word = r.u16()?;
    let pixmap = row_word & 0x8000 != 0;
    if row_word & 0x4000 != 0 {
        return Err(unsupported("reserved rowBytes flag"));
    }
    let row_bytes = (row_word & 0x3fff) as usize;
    let bounds = r.rect()?;
    let (width, height) = (bounds.width(), bounds.height());
    if width == 0 || height == 0 || row_bytes == 0 || row_bytes % 2 != 0 {
        return Err(malformed("invalid bitmap dimensions or rowBytes"));
    }
    budget.limits.pixels(width, height)?;
    let (mut pack_type, mut depth, mut components) = (0, 1u16, 1u16);
    let mut palette = [None; 256];
    if pixmap {
        if version == 1 {
            return Err(unsupported("indexed PixMaps in PICT v1"));
        }
        let pm_version = r.u16()?;
        pack_type = r.u16()?;
        r.u32()?;
        r.u32()?;
        r.u32()?;
        let pixel_type = r.u16()?;
        depth = r.u16()?;
        components = r.u16()?;
        let component_size = r.u16()?;
        let plane_bytes = r.u32()?;
        r.u32()?;
        r.u32()?;
        if pm_version != 0 || plane_bytes != 0 {
            return Err(unsupported("PixMap version or planar layout"));
        }
        if direct {
            if pixel_type != 16
                || !matches!(
                    (depth, components, component_size),
                    (16, 3, 5) | (32, 3 | 4, 8)
                )
            {
                return Err(unsupported("direct pixel component layout"));
            }
        } else {
            if pixel_type != 0
                || !matches!(depth, 1 | 2 | 4 | 8)
                || components != 1
                || component_size != depth
            {
                return Err(unsupported("indexed pixel component layout"));
            }
            if pack_type > 1 {
                return Err(unsupported("indexed packing type"));
            }
            r.u32()?;
            let flags = r.u16()?;
            let entries = r.u16()? as usize + 1;
            if entries > 256 {
                return Err(malformed("color table contains more than 256 entries"));
            }
            for i in 0..entries {
                let value = r.u16()? as usize;
                let color = r.color()?;
                let index = if flags & 0x8000 != 0 { i } else { value };
                if index >= (1usize << depth) || palette[index].is_some() {
                    return Err(malformed("invalid or duplicate color table index"));
                }
                palette[index] = Some(color);
            }
        }
    } else if direct {
        return Err(malformed("direct bitmap lacks PixMap flag"));
    }
    if width * depth as usize > row_bytes * 8 {
        return Err(malformed("rowBytes smaller than bitmap width"));
    }
    Ok(MapLayout {
        bounds,
        row_bytes,
        pixmap,
        direct,
        pack_type,
        depth,
        components,
        palette,
    })
}

fn map_pixels(
    r: &mut Reader<'_>,
    layout: MapLayout,
    packed_opcode: bool,
    budget: &mut Budget<'_>,
) -> Result<Pixels> {
    let MapLayout {
        bounds,
        row_bytes,
        pixmap,
        direct,
        mut pack_type,
        depth,
        components,
        palette,
    } = layout;
    let (width, height) = (bounds.width(), bounds.height());
    let pixels = budget.limits.pixels(width, height)?;
    if direct && pack_type == 0 {
        pack_type = if depth == 16 { 3 } else { 4 };
    }
    if direct && !matches!((depth, pack_type), (16, 1 | 3) | (32, 1 | 2 | 4)) {
        return Err(unsupported("direct pixel packing type"));
    }
    let packed = row_bytes >= 8
        && if direct {
            pack_type >= 3
        } else {
            packed_opcode && pack_type != 1
        };
    let row_size = if direct && row_bytes >= 8 && depth == 32 && pack_type == 4 {
        row_bytes / 4 * components as usize
    } else if direct && row_bytes >= 8 && depth == 32 && pack_type == 2 {
        row_bytes / 4 * 3
    } else {
        row_bytes
    };
    if direct && depth == 32 && row_bytes % 4 != 0 {
        return Err(malformed("32-bit rowBytes is not pixel aligned"));
    }
    budget.allocate(
        row_size
            .checked_mul(height)
            .ok_or_else(|| malformed("bitmap size overflow"))?,
    )?;
    budget.allocate(pixels * if pixmap { 3 } else { 1 })?;
    budget.work((pixels + row_size * height) as u64)?;
    let mut decoded = Vec::with_capacity(pixels * if pixmap { 3 } else { 1 });
    for _ in 0..height {
        let row = if packed {
            let encoded = if row_bytes > 250 {
                r.u16()? as usize
            } else {
                r.u8()? as usize
            };
            let start = r.offset();
            unpack(
                r.take(encoded)?,
                row_size,
                if direct && depth == 16 { 2 } else { 1 },
            )
            .map_err(|mut e| {
                e.offset = Some(start);
                e
            })?
        } else {
            r.take(row_size)?.to_vec()
        };
        for x in 0..width {
            if !pixmap {
                decoded.push((row[x / 8] >> (7 - x % 8)) & 1);
            } else if !direct {
                let bit = x * depth as usize;
                let index = ((row[bit / 8] >> (8 - depth as usize - bit % 8))
                    & ((1u16 << depth) - 1) as u8) as usize;
                decoded.extend_from_slice(
                    &palette[index]
                        .ok_or_else(|| malformed("pixel references absent color table entry"))?,
                );
            } else if depth == 16 {
                let pixel = u16::from_be_bytes([row[x * 2], row[x * 2 + 1]]);
                for shift in [10, 5, 0] {
                    let v = ((pixel >> shift) & 31) as u8;
                    decoded.push((v << 3) | (v >> 2));
                }
            } else if packed && pack_type == 4 {
                let stride = row_bytes / 4;
                let start = if components == 4 { stride } else { 0 };
                decoded.extend_from_slice(&[
                    row[start + x],
                    row[start + stride + x],
                    row[start + stride * 2 + x],
                ]);
            } else {
                let stride = if pack_type == 2 && row_bytes >= 8 {
                    3
                } else {
                    4
                };
                let start = x * stride + if stride == 4 { 1 } else { 0 };
                decoded.extend_from_slice(&row[start..start + 3]);
            }
        }
    }
    Ok(if pixmap {
        Pixels::Rgb(decoded)
    } else {
        Pixels::Mono(decoded)
    })
}

fn pixel_pattern(r: &mut Reader<'_>, budget: &mut Budget<'_>) -> Result<Pattern> {
    let kind = r.u16()?;
    let old = r.pattern()?;
    match kind {
        0 => Ok(Pattern::Bits(old)),
        2 => Ok(Pattern::Solid(r.color()?)),
        1 => {
            let layout = map_layout(r, false, 2, budget)?;
            if !layout.pixmap {
                return Err(malformed("pixel pattern requires a PixMap"));
            }
            let bounds = layout.bounds;
            if !bounds.width().is_power_of_two() || !bounds.height().is_power_of_two() {
                return Err(malformed("pixel pattern dimensions must be powers of two"));
            }
            match map_pixels(r, layout, true, budget)? {
                Pixels::Rgb(rgb) => Ok(Pattern::Color { bounds, rgb }),
                Pixels::Mono(_) => unreachable!(),
            }
        }
        _ => Err(unsupported("unknown pixel pattern type")),
    }
}
