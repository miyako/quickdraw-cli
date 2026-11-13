use crate::model::*;
use crate::{FontFamily, FontResourceType, list_font_resources};
use serde::Deserialize;
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
    fractional_widths: Option<usize>,
    image_heights: Option<usize>,
    family: Option<crate::fond::FamilyMetrics>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontResourceSelection {
    #[serde(rename = "type")]
    pub resource_type: FontResourceType,
    pub id: i16,
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontScript {
    Roman,
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontFamilySelection {
    pub id: i16,
    pub script: FontScript,
    #[serde(default)]
    pub kerning: bool,
}
#[derive(Clone, Debug, Default)]
pub struct FontSelection {
    pub resource: Option<FontResourceSelection>,
    pub family: Option<FontFamilySelection>,
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
        if flags & 0x0ffc != 0 {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "NFNT color/depth, synthetic fonts and reserved low flag bits are unsupported",
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
        let mut end = widths + count * 2;
        // Optional tables have one entry per glyph, including missing, but no terminator.
        let fractional_widths = (flags & 2 != 0).then(|| {
            let start = end;
            end += (count - 1) * 2;
            start
        });
        let image_heights = (flags & 1 != 0).then(|| {
            let start = end;
            end += (count - 1) * 2;
            start
        });
        if widths < locations + count * 2 || end != data.len() {
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
        if let Some(table) = image_heights {
            let mut budget = Budget::new(limits);
            budget.work((row_bytes * 8 * height) as u64)?;
            for i in 0..count - 1 {
                let entry = word(data, table + i * 2)?;
                let first_row = usize::from(entry >> 8);
                let last_row = first_row + usize::from(entry & 255);
                if last_row > height {
                    return Err(Error::new(
                        ErrorCode::Malformed,
                        "NFNT image-height extent exceeds font rectangle",
                    ));
                }
                let left = usize::from(word(data, locations + i * 2)?);
                let right = usize::from(word(data, locations + (i + 1) * 2)?);
                if left == right {
                    continue;
                }
                for y in (0..first_row).chain(last_row..height) {
                    for x in left..right {
                        if data[26 + y * row_bytes + x / 8] & (0x80 >> (x % 8)) != 0 {
                            return Err(Error::new(
                                ErrorCode::Malformed,
                                "NFNT image-height table excludes glyph ink",
                            ));
                        }
                    }
                }
            }
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
            fractional_widths,
            image_heights,
            family: None,
        })
    }

    pub fn from_container(
        data: &[u8],
        container: Container,
        size: u16,
        face: u8,
        selection: &FontSelection,
        limits: &Limits,
    ) -> Result<Self> {
        let invalid = |message| Error::new(ErrorCode::InvalidArguments, message);
        if selection.resource.is_none() && selection.family.is_none() {
            return Err(invalid(
                "font container requires an explicit resource or family selection",
            ));
        }
        if selection
            .resource
            .is_some_and(|s| s.resource_type == FontResourceType::Fond)
        {
            return Err(invalid(
                "bitmap resource type must be NFNT or FONT; select FOND with family",
            ));
        }
        let inventory = list_font_resources(data, container, limits)?;
        let family = selection.family.map(|selected| -> Result<_> {
            let entry = inventory.resources.iter().find(|entry|
                entry.resource_type == FontResourceType::Fond && entry.resource_id == selected.id
            ).ok_or_else(|| invalid("selected FOND resource is missing"))?;
            let family = FontFamily::parse(&data[entry.offset..entry.offset + entry.length], limits)?;
            if family.family_id != selected.id {
                return Err(Error::new(ErrorCode::Malformed, "FOND header family ID differs from resource ID"));
            }
            match selected.script {
                FontScript::Roman if (0..16384).contains(&family.family_id) => {}
                _ => return Err(Error::new(ErrorCode::Unsupported, "only explicitly declared Roman FOND families with IDs 0..16383 are supported")),
            }
            Ok(family)
        }).transpose()?;
        let associated = family
            .as_ref()
            .map(|f| f.resource_id(size, face))
            .transpose()?;
        if let (Some(id), Some(resource)) = (associated, selection.resource)
            && id != resource.id
        {
            return Err(invalid(
                "bitmap resource ID conflicts with exact FOND association",
            ));
        }
        let mut choices = inventory.resources.iter().filter(|entry| {
            entry.resource_type != FontResourceType::Fond
                && associated.is_none_or(|id| entry.resource_id == id)
                && selection.resource.is_none_or(|s| {
                    entry.resource_id == s.id && entry.resource_type == s.resource_type
                })
        });
        let entry = choices
            .next()
            .ok_or_else(|| invalid("selected bitmap font resource is missing"))?;
        if choices.next().is_some() {
            return Err(invalid(
                "ambiguous NFNT/FONT association; select resource type and ID explicitly",
            ));
        }
        let mut font = Self::parse(&data[entry.offset..entry.offset + entry.length], limits)?;
        if let (Some(family), Some(selected)) = (family, selection.family) {
            font.family = Some(family.metrics(
                size,
                face,
                selected.kerning,
                font.fractional_widths.is_none(),
            )?);
        }
        Ok(font)
    }

    pub(crate) fn kerning_enabled(&self) -> bool {
        self.family.as_ref().is_some_and(|f| f.kerning_enabled)
    }
    pub(crate) fn kern(&self, first: u8, second: u8) -> i64 {
        self.family.as_ref().map_or(0, |f| f.kern(first, second))
    }

    pub(crate) fn glyph(&self, code: u8, fractional: bool) -> Result<Glyph<'_>> {
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
        let rows = match self.image_heights {
            Some(table) => {
                let entry = word(&self.data, table + index * 2)?;
                usize::from(entry >> 8)..usize::from(entry >> 8) + usize::from(entry & 255)
            }
            None => 0..self.height,
        };
        let fractional_advance = self
            .fractional_widths
            .map(|table| word(&self.data, table + index * 2).map(|v| i64::from(v) * 256))
            .transpose()?;
        let fractional_advance = match (fractional_advance, &self.family) {
            (None, Some(family)) if fractional => family.advance(code)?,
            (value, _) => value,
        };
        Ok(Glyph {
            font: self,
            left,
            width: right - left,
            height: self.height,
            bearing: self.kern + (metrics >> 8) as i32,
            advance: (metrics & 255) as i32,
            fractional_advance,
            ascent: self.ascent,
            rows,
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
    pub fractional_advance: Option<i64>,
    pub ascent: i32,
    rows: std::ops::Range<usize>,
}
impl Glyph<'_> {
    pub fn ink(&self, x: usize, y: usize) -> bool {
        if !self.rows.contains(&y) {
            return false;
        }
        let bit = self.left + x;
        self.font.data[26 + y * self.font.row_bytes + bit / 8] & (0x80 >> (bit % 8)) != 0
    }
}

/// Explicit bindings for source font ID, source point size, and recorded style byte.
/// No system search, size selection, synthetic styling, or missing-glyph fallback occurs.
#[derive(Clone, Debug, Default)]
pub struct FontBook {
    fonts: BTreeMap<(i16, u16, u8), BitmapFont>,
    outlines: BTreeMap<(i16, u16, u8), crate::OutlineFont>,
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
        if font
            .family
            .as_ref()
            .is_some_and(|f| f.size != size || f.face != face)
        {
            return Err(Error::new(
                ErrorCode::InvalidArguments,
                "binding size/style conflicts with selected FOND strike",
            ));
        }
        if self.fonts.contains_key(&(id, size, face))
            || self.outlines.contains_key(&(id, size, face))
        {
            return Err(Error::new(
                ErrorCode::InvalidArguments,
                "duplicate font binding",
            ));
        }

        let bytes = self
            .bytes
            .checked_add(font.data.len())
            .and_then(|n| n.checked_add(font.family.as_ref().map_or(0, |f| f.byte_cost)))
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
    pub fn insert_outline(
        &mut self,
        id: i16,
        size: u16,
        face: u8,
        font: crate::OutlineFont,
        limits: &Limits,
    ) -> Result<()> {
        limits.validate()?;
        if size == 0 || face & !3 != 0 {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "outline bindings require a positive size and intrinsic regular/bold/italic face; synthetic styles are unsupported",
            ));
        }
        if face != font.style() {
            return Err(Error::new(
                ErrorCode::InvalidArguments,
                "outline binding style does not match the supplied face",
            ));
        }
        if self.fonts.contains_key(&(id, size, face))
            || self.outlines.contains_key(&(id, size, face))
        {
            return Err(Error::new(
                ErrorCode::InvalidArguments,
                "duplicate font binding",
            ));
        }
        self.bytes = self
            .bytes
            .checked_add(font.byte_len())
            .filter(|&n| n <= limits.max_decoded_bytes)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::ResourceLimit,
                    "font collection byte limit exceeded",
                )
            })?;
        self.outlines.insert((id, size, face), font);
        Ok(())
    }
    pub(crate) fn outline(&self, id: i16, size: u16, face: u8) -> Option<&crate::OutlineFont> {
        self.outlines.get(&(id, size, face))
    }
    pub(crate) fn get(&self, id: i16, size: u16, face: u8) -> Result<&BitmapFont> {
        self.fonts.get(&(id, size, face)).ok_or_else(|| {
            Error::new(
                ErrorCode::Unsupported,
                format!(
                    "no explicit NFNT or outline font binding for id={id}, size={size}, face={face}"
                ),
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
    ratio: [i16; 4],
    pub fraction: u16,
    pub fractional_widths: bool,
    unsupported_justify: bool,
    unsupported_glyphs: bool,
    preserve_glyphs: bool,
    scaling_disabled: bool,
}
impl TextState {
    pub fn new() -> Self {
        Self {
            mode: 1,
            ratio: [1; 4],
            fraction: 0x8000,
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
                for (i, value) in self.ratio.iter_mut().enumerate() {
                    *value = word(data, i * 2)? as i16;
                }
                if self.ratio[2] == 0 || self.ratio[3] == 0 {
                    return Err(Error::new(
                        ErrorCode::Malformed,
                        "zero text ratio denominator",
                    ));
                }
            }
            0x15 => self.fraction = word(data, 0)?,
            0x16 => self.extra = word(data, 0)? as i16 as i32,
            0x2d => self.unsupported_justify = data.iter().any(|&b| b != 0),
            0x2e => {
                if data.iter().any(|&b| b > 1) {
                    return Err(Error::new(
                        ErrorCode::Malformed,
                        "glyphState requires Boolean bytes",
                    ));
                }
                self.unsupported_glyphs = data[0] != 0;
                self.preserve_glyphs = data[1] != 0;
                self.fractional_widths = data[2] != 0;
                self.scaling_disabled = data[3] != 0;
                // PreserveGlyph only changes outline glyphs, not an explicit bitmap strike.
            }
            _ => unreachable!("parsed text state"),
        }
        Ok(())
    }
    pub fn check_for(&self, outline: bool) -> Result<()> {
        if self.unsupported_justify {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "nonzero lineJustify is unsupported: the recorded extra-character and total-run widths lack a verified replay mapping to SpExtra/ChExtra and scaling",
            ));
        }
        if self.ratio.iter().any(|&n| n <= 0)
            || (!outline && self.unsupported_glyphs)
            || (outline && self.preserve_glyphs)
            || (self.scaling_disabled && self.scaled())
        {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "nonpositive/disabled text scaling, bitmap outline preference or outline preserveGlyph request is unsupported",
            ));
        }
        if !matches!(self.base_mode(), 0..=7 | 32..=39) || self.mode & !0x7f != 0 {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "text supports source modes 0-7 and 32-39, optionally combined with text mask 64",
            ));
        }
        Ok(())
    }
    pub fn spacing(&self, code: u8) -> i64 {
        if code == 32 {
            i64::from(self.space)
        } else {
            i64::from(self.extra) * 65536
        }
    }
    pub fn base_mode(&self) -> u16 {
        self.mode & !64
    }
    pub fn masked(&self) -> bool {
        self.mode & 64 != 0
    }
    pub fn portable_layout(&self) -> bool {
        self.scaled()
            || self.space & 0xffff != 0
            || !matches!(self.fraction, 0 | 0x8000)
            || self.fractional_widths
    }
    fn scaled(&self) -> bool {
        self.ratio[0] != self.ratio[2] || self.ratio[1] != self.ratio[3]
    }
    pub fn scale(&self, fixed: i64, horizontal: bool) -> Result<i64> {
        let i = usize::from(horizontal);
        fixed
            .checked_mul(i64::from(self.ratio[i]))
            .map(|v| v.div_euclid(i64::from(self.ratio[i + 2])))
            .ok_or_else(|| Error::new(ErrorCode::ResourceLimit, "text scaling overflow"))
    }
}
