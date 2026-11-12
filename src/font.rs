use crate::model::*;
use std::{collections::BTreeMap, sync::Arc};

/// An extracted one-bit NFNT/FONT resource, not a resource-fork container.
#[derive(Debug, Clone)]
pub struct BitmapFont {
    data: Arc<[u8]>,
    first: u16,
    last: u16,
    row_bytes: usize,
    height: usize,
    ascent: i32,
    kern: i32,
    locations: usize,
    widths: usize,
}

fn word(data: &[u8], at: usize) -> Result<u16> {
    let bytes = data
        .get(at..at + 2)
        .ok_or_else(|| Error::new(ErrorCode::Malformed, "truncated NFNT").at(at))?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

impl BitmapFont {
    pub fn parse(data: &[u8], limits: &Limits) -> Result<Self> {
        limits.validate()?;
        if data.len() > limits.max_input_bytes || data.len() > limits.max_decoded_bytes {
            return Err(Error::new(
                ErrorCode::ResourceLimit,
                "font byte limit exceeded",
            ));
        }
        let flags = word(data, 0)?;
        if flags & 0x0fff != 0 {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "NFNT color, fractional-width and optional-height tables are not implemented",
            ));
        }
        let (first, last) = (word(data, 2)?, word(data, 4)?);
        if first > last || last > 255 {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "NFNT must use single-byte character codes",
            ));
        }
        let height = word(data, 14)? as usize;
        let ascent = word(data, 18)? as i32;
        let descent = word(data, 20)? as i32;
        let row_bytes = word(data, 24)? as usize * 2;
        if height == 0 || row_bytes == 0 || ascent + descent != height as i32 {
            return Err(Error::new(
                ErrorCode::Malformed,
                "invalid NFNT bitmap dimensions or ascent/descent",
            ));
        }
        limits.pixels(row_bytes * 8, height)?;
        let locations = 26 + row_bytes * height;
        let count = usize::from(last - first) + 3;
        let high = (word(data, 10)? as i16).max(0) as usize;
        let widths = 16 + ((high << 16) | word(data, 16)? as usize) * 2;
        if widths < locations + count * 2 || widths.checked_add(count * 2) != Some(data.len()) {
            return Err(Error::new(
                ErrorCode::Malformed,
                "NFNT width table offset or extent is inconsistent",
            ));
        }
        let mut previous = 0;
        for i in 0..count {
            let location = word(data, locations + i * 2)? as usize;
            if location < previous || location > row_bytes * 8 {
                return Err(Error::new(
                    ErrorCode::Malformed,
                    "NFNT glyph locations are unordered or out of bounds",
                ));
            }
            previous = location;
        }
        if word(data, widths + (count - 1) * 2)? != 0xffff {
            return Err(Error::new(
                ErrorCode::Malformed,
                "NFNT width table missing terminator",
            ));
        }
        Ok(Self {
            data: data.into(),
            first,
            last,
            row_bytes,
            height,
            ascent,
            kern: word(data, 8)? as i16 as i32,
            locations,
            widths,
        })
    }

    pub(crate) fn glyph(&self, code: u8) -> Result<Glyph<'_>> {
        if u16::from(code) < self.first || u16::from(code) > self.last {
            return Err(Error::new(
                ErrorCode::Unsupported,
                format!("font has no glyph for byte 0x{code:02x}; replacement is disabled"),
            ));
        }
        let index = usize::from(u16::from(code) - self.first);
        let metrics = word(&self.data, self.widths + index * 2)?;
        if metrics == 0xffff {
            return Err(Error::new(
                ErrorCode::Unsupported,
                format!("font marks byte 0x{code:02x} missing; replacement is disabled"),
            ));
        }
        let left = word(&self.data, self.locations + index * 2)? as usize;
        let right = word(&self.data, self.locations + (index + 1) * 2)? as usize;
        Ok(Glyph {
            font: self,
            left,
            width: right - left,
            height: self.height,
            bearing: self.kern + (metrics >> 8) as i32,
            advance: (metrics & 255) as i32,
            ascent: self.ascent,
        })
    }
}

pub(crate) struct Glyph<'a> {
    font: &'a BitmapFont,
    left: usize,
    pub width: usize,
    pub height: usize,
    pub bearing: i32,
    pub advance: i32,
    pub ascent: i32,
}
impl Glyph<'_> {
    pub fn ink(&self, x: usize, y: usize) -> bool {
        let bit = self.left + x;
        self.font.data[26 + y * self.font.row_bytes + bit / 8] & (0x80 >> (bit % 8)) != 0
    }
}

/// Explicit bindings for source font ID, source point size, and recorded style byte.
/// No system search, size selection, synthetic styling, or missing-glyph fallback occurs.
#[derive(Clone, Debug, Default)]
pub struct FontBook {
    fonts: BTreeMap<(i16, u16, u8), BitmapFont>,
    bytes: usize,
}
impl FontBook {
    pub fn insert(
        &mut self,
        id: i16,
        size: u16,
        face: u8,
        font: BitmapFont,
        limits: &Limits,
    ) -> Result<()> {
        if self.fonts.contains_key(&(id, size, face)) {
            return Err(Error::new(
                ErrorCode::InvalidArguments,
                "duplicate font binding",
            ));
        }

        let bytes = self
            .bytes
            .checked_add(font.data.len())
            .filter(|&n| n <= limits.max_decoded_bytes)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::ResourceLimit,
                    "font collection byte limit exceeded",
                )
            })?;
        self.fonts.insert((id, size, face), font);
        self.bytes = bytes;
        Ok(())
    }
    pub(crate) fn get(&self, id: i16, size: u16, face: u8) -> Result<&BitmapFont> {
        self.fonts.get(&(id, size, face)).ok_or_else(|| {
            Error::new(
                ErrorCode::Unsupported,
                format!("no explicit NFNT font binding for id={id}, size={size}, face={face}"),
            )
        })
    }
}

#[derive(Default)]
pub(crate) struct TextState {
    pub id: i16,
    pub size: u16,
    pub face: u8,
    pub mode: u16,
    space: i32,
    extra: i32,
    unsupported_ratio: bool,
    unsupported_justify: bool,
    unsupported_glyphs: bool,
    unsupported_fraction: bool,
}
impl TextState {
    pub fn new() -> Self {
        Self {
            mode: 1,
            ..Default::default()
        }
    }
    pub fn update(&mut self, opcode: u16, data: &[u8]) -> Result<()> {
        match opcode {
            3 => self.id = word(data, 0)? as i16,
            4 => self.face = data[0],
            5 => self.mode = word(data, 0)?,
            6 => self.space = i32::from_be_bytes(data.try_into().expect("parsed Fixed")),
            0x0d => self.size = word(data, 0)?,
            0x10 => {
                self.unsupported_ratio = word(data, 0)? == 0
                    || word(data, 2)? == 0
                    || word(data, 0)? != word(data, 4)?
                    || word(data, 2)? != word(data, 6)?;
            }
            0x15 => self.unsupported_fraction = !matches!(word(data, 0)?, 0 | 0x8000),
            0x16 => self.extra = word(data, 0)? as i16 as i32,
            0x2d => self.unsupported_justify = data.iter().any(|&b| b != 0),
            0x2e => self.unsupported_glyphs = data[0] != 0 || data[2] != 0,
            _ => unreachable!("parsed text state"),
        }
        Ok(())
    }
    pub fn check(&self) -> Result<()> {
        if self.unsupported_ratio
            || self.unsupported_justify
            || self.unsupported_glyphs
            || self.unsupported_fraction
            || self.space & 0xffff != 0
        {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "NFNT text scaling, fractional positioning/spacing, justification or outline preference requires an unimplemented font layout policy",
            ));
        }
        if !matches!(self.mode, 1 | 3) {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "NFNT text currently supports srcOr and srcBic only",
            ));
        }
        Ok(())
    }
    pub fn spacing(&self, code: u8) -> i32 {
        if code == 32 {
            self.space >> 16
        } else {
            self.extra
        }
    }
}
