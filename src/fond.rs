use crate::model::*;
use std::collections::{BTreeMap, BTreeSet};

fn malformed(message: &str) -> Error {
    Error::new(ErrorCode::Malformed, message)
}
fn unsupported(message: &str) -> Error {
    Error::new(ErrorCode::Unsupported, message)
}
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn word(&mut self) -> Result<u16> {
        let bytes = self
            .data
            .get(self.at..self.at + 2)
            .ok_or_else(|| malformed("truncated FOND table"))?;
        self.at += 2;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }
    fn long(&mut self) -> Result<usize> {
        Ok(((self.word()? as usize) << 16) | self.word()? as usize)
    }
    fn count(&mut self) -> Result<usize> {
        let count = self.word()?;
        Ok(if count == 0xffff {
            0
        } else {
            count as usize + 1
        })
    }
    fn finish(&self) -> Result<()> {
        if self.data[self.at..].iter().any(|&b| b != 0) {
            return Err(malformed("unexpected data after FOND table"));
        }
        Ok(())
    }
}

/// Family tables for an explicitly selected intrinsic bitmap strike.
/// Printer names, outline metrics and synthetic style properties are not applied.
#[derive(Debug)]
pub struct FontFamily {
    pub family_id: i16,
    first: u16,
    last: u16,
    flags: u16,
    version: u16,
    associations: BTreeMap<(u16, u16), i16>,
    widths: BTreeMap<u16, Vec<u16>>,
    kerning: BTreeMap<u16, BTreeMap<(u8, u8), u16>>,
    byte_cost: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct FamilyMetrics {
    pub size: u16,
    pub face: u8,
    first: u16,
    widths: Option<Vec<i64>>,
    kerning: BTreeMap<(u8, u8), i64>,
    pub kerning_enabled: bool,
    pub byte_cost: usize,
}
impl FamilyMetrics {
    pub fn advance(&self, code: u8) -> Result<Option<i64>> {
        self.widths
            .as_ref()
            .map(|widths| {
                u16::from(code)
                    .checked_sub(self.first)
                    .and_then(|i| widths.get(usize::from(i)))
                    .copied()
                    .ok_or_else(|| unsupported("glyph lies outside selected FOND width range"))
            })
            .transpose()
    }
    pub fn kern(&self, first: u8, second: u8) -> i64 {
        self.kerning.get(&(first, second)).copied().unwrap_or(0)
    }
}

impl FontFamily {
    pub fn parse(data: &[u8], limits: &Limits) -> Result<Self> {
        limits.validate()?;
        if data.len() > limits.max_input_bytes {
            return Err(Error::new(
                ErrorCode::ResourceLimit,
                "FOND byte limit exceeded",
            ));
        }
        let mut budget = Budget::new(limits);
        budget.allocate(data.len())?;
        budget.work(data.len() as u64)?;
        let mut byte_cost = data.len();
        let mut records = 0usize;
        let mut charge = |count: usize| -> Result<()> {
            records = records
                .checked_add(count)
                .filter(|&n| n <= limits.max_operations)
                .ok_or_else(|| {
                    Error::new(ErrorCode::ResourceLimit, "FOND entry count limit exceeded")
                })?;
            let bytes = count
                .checked_mul(64)
                .ok_or_else(|| malformed("FOND allocation overflow"))?;
            budget.allocate(bytes)?;
            budget.work(count as u64)?;
            byte_cost += bytes;
            Ok(())
        };
        let mut r = Reader { data, at: 0 };
        let flags = r.word()?;
        if flags & 0x0ffd != 0 {
            return Err(unsupported("reserved FOND flags are unsupported"));
        }
        let family_id = r.word()? as i16;
        let first = r.word()?;
        let last = r.word()?;
        if first > last || last > 255 {
            return Err(unsupported("FOND must use single-byte character codes"));
        }
        r.at = 16;
        let width_at = r.long()?;
        let kern_at = r.long()?;
        let style_at = r.long()?;
        r.at = 50;
        let version = r.word()?;
        let n = r.count()?;
        charge(n)?;
        let mut associations = BTreeMap::new();
        for _ in 0..n {
            let size = r.word()?;
            let style = r.word()?;
            let id = r.word()? as i16;
            if associations.insert((size, style), id).is_some() {
                return Err(malformed("duplicate FOND size/style association"));
            }
        }
        let association_end = r.at;
        let mut starts = BTreeSet::new();
        for at in [width_at, kern_at, style_at]
            .into_iter()
            .filter(|&n| n != 0)
        {
            if at < association_end || at >= data.len() || at % 2 != 0 || !starts.insert(at) {
                return Err(malformed("invalid or overlapping FOND table offsets"));
            }
        }
        // Later FONDs have a directory relative to the end of the association table.
        // Earlier layouts can start their first named table immediately after it.
        if association_end < starts.first().copied().unwrap_or(data.len()) {
            let n = r.count()?;
            charge(n)?;
            let directory_end =
                r.at.checked_add(n * 4)
                    .filter(|&end| end <= data.len())
                    .ok_or_else(|| malformed("truncated FOND offset directory"))?;
            if starts.first().is_some_and(|&at| at < directory_end) {
                return Err(malformed("FOND directory overlaps named table"));
            }
            let mut offsets = BTreeSet::new();
            for _ in 0..n {
                let at = association_end
                    .checked_add(r.long()?)
                    .ok_or_else(|| malformed("FOND directory offset overflow"))?;
                if at < directory_end || at >= data.len() || at % 2 != 0 || !offsets.insert(at) {
                    return Err(malformed("invalid FOND directory offset"));
                }
                starts.insert(at);
            }
        }
        let table = |at: usize| Reader {
            data: &data[at..starts.range(at + 1..).next().copied().unwrap_or(data.len())],
            at: 0,
        };
        let mut widths = BTreeMap::new();
        if width_at != 0 {
            let mut r = table(width_at);
            let n = r.count()?;
            charge(n)?;
            for _ in 0..n {
                let style = r.word()?;
                let count = usize::from(last - first) + 2; // glyphs plus missing glyph
                charge(count)?;
                let values = (0..count).map(|_| r.word()).collect::<Result<Vec<_>>>()?;
                if r.word()? != 0 {
                    return Err(malformed("FOND width table missing zero terminator"));
                }
                if widths.insert(style, values).is_some() {
                    return Err(malformed("duplicate FOND width style"));
                }
            }
            r.finish()?;
        }
        let mut kerning = BTreeMap::new();
        if kern_at != 0 {
            let mut r = table(kern_at);
            let n = r.count()?;
            charge(n)?;
            for _ in 0..n {
                let style = r.word()?;
                // Adobe TN 0091 figure 10: this is a pair count, not a byte length.
                let pairs = r.word()? as usize;
                charge(pairs)?;
                let mut values = BTreeMap::new();
                for _ in 0..pairs {
                    let codes = r.word()?;
                    let pair = ((codes >> 8) as u8, codes as u8);
                    let value = r.word()?;
                    if values.insert(pair, value).is_some() {
                        return Err(malformed("duplicate FOND kerning pair"));
                    }
                }
                if kerning.insert(style, values).is_some() {
                    return Err(malformed("duplicate FOND kerning style"));
                }
            }
            r.finish()?;
        }
        Ok(Self {
            family_id,
            first,
            last,
            flags,
            version,
            associations,
            widths,
            kerning,
            byte_cost,
        })
    }

    pub fn resource_id(&self, size: u16, face: u8) -> Result<i16> {
        if size == 0 || face & 0x80 != 0 {
            return Err(unsupported(
                "FOND selection requires a nonzero-size intrinsic monochrome style; sfnt is unsupported",
            ));
        }
        self.associations
            .get(&(size, u16::from(face)))
            .copied()
            .ok_or_else(|| {
                unsupported(
                    "no exact monochrome FOND size/style association; substitution is disabled",
                )
            })
    }

    pub(crate) fn metrics(
        &self,
        size: u16,
        face: u8,
        kerning: bool,
        use_widths: bool,
    ) -> Result<FamilyMetrics> {
        self.resource_id(size, face)?;
        let fixed = |value: u16| -> Result<i64> {
            if value & 0x8000 != 0 && self.version < 2 {
                return Err(unsupported(
                    "negative FOND version 0/1 fixed values require an explicit historical representation contract",
                ));
            }
            Ok(i64::from(value as i16) * i64::from(size) * 16)
        };
        let widths = if use_widths && self.flags & 0x4000 == 0 && !self.widths.is_empty() {
            let values = self.widths.get(&u16::from(face)).ok_or_else(|| {
                unsupported("no exact FOND width style; synthetic style metrics are disabled")
            })?;
            Some(
                values[..usize::from(self.last - self.first) + 1]
                    .iter()
                    .map(|&v| fixed(v))
                    .collect::<Result<Vec<_>>>()?,
            )
        } else {
            None
        };
        let pairs = if kerning {
            self.kerning
                .get(&u16::from(face))
                .ok_or_else(|| unsupported("requested FOND kerning has no exact style table"))?
                .iter()
                .map(|(&pair, &value)| Ok((pair, fixed(value)?)))
                .collect::<Result<BTreeMap<_, _>>>()?
        } else {
            BTreeMap::new()
        };
        Ok(FamilyMetrics {
            size,
            face,
            first: self.first,
            widths,
            kerning: pairs,
            kerning_enabled: kerning,
            byte_cost: self.byte_cost,
        })
    }
}
