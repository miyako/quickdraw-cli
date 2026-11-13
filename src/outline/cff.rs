use super::{limit, malformed, unsupported};
use crate::model::{Budget, Result};

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&end| end <= self.data.len())
            .ok_or_else(|| malformed("truncated CFF structure"))?;
        let result = &self.data[self.at..end];
        self.at = end;
        Ok(result)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn integer(&mut self, n: usize) -> Result<usize> {
        Ok(self
            .take(n)?
            .iter()
            .fold(0, |v, &b| (v << 8) | usize::from(b)))
    }
    fn index(&mut self, budget: &mut Budget<'_>) -> Result<Vec<&'a [u8]>> {
        let count = self.integer(2)?;
        if count == 0 {
            return Ok(Vec::new());
        }
        if count > budget.limits.max_operations {
            return Err(limit("CFF INDEX count exceeded"));
        }
        budget.allocate(count * std::mem::size_of::<&[u8]>())?;
        budget.work(count as u64)?;
        let size = self.byte()? as usize;
        if !(1..=4).contains(&size) {
            return Err(malformed("invalid CFF INDEX offset size"));
        }
        let offsets = self.take((count + 1) * size)?;
        let value = |i: usize| {
            offsets[i * size..(i + 1) * size]
                .iter()
                .fold(0usize, |v, &b| (v << 8) | usize::from(b))
        };
        if value(0) != 1 {
            return Err(malformed("CFF INDEX must start at offset one"));
        }
        let last = value(count);
        if last == 0 {
            return Err(malformed("zero CFF INDEX end offset"));
        }
        let data = self.take(last - 1)?;
        let mut result = Vec::with_capacity(count);
        for i in 0..count {
            let (start, end) = (value(i), value(i + 1));
            if start == 0 || start > end || end > last {
                return Err(malformed("invalid CFF INDEX extent"));
            }
            result.push(&data[start - 1..end - 1]);
        }
        Ok(result)
    }
    fn number(&mut self, b: u8, dictionary: bool) -> Result<f64> {
        Ok(match b {
            32..=246 => f64::from(b) - 139.0,
            247..=250 => f64::from((u16::from(b) - 247) * 256 + u16::from(self.byte()?) + 108),
            251..=254 => -f64::from((u16::from(b) - 251) * 256 + u16::from(self.byte()?) + 108),
            28 => f64::from(i16::from_be_bytes(self.take(2)?.try_into().unwrap())),
            29 if dictionary => f64::from(i32::from_be_bytes(self.take(4)?.try_into().unwrap())),
            255 if !dictionary => {
                f64::from(i32::from_be_bytes(self.take(4)?.try_into().unwrap())) / 65536.0
            }
            30 if dictionary => {
                let mut text = String::new();
                'digits: loop {
                    let v = self.byte()?;
                    for digit in [v >> 4, v & 15] {
                        match digit {
                            0..=9 => text.push(char::from(b'0' + digit)),
                            10 => text.push('.'),
                            11 => text.push('E'),
                            12 => text.push_str("E-"),
                            14 => text.push('-'),
                            15 => break 'digits,
                            _ => return Err(malformed("invalid CFF real digit")),
                        }
                        if text.len() > 64 {
                            return Err(limit("CFF real operand length exceeded"));
                        }
                    }
                }
                let value = text
                    .parse::<f64>()
                    .map_err(|_| malformed("invalid CFF real operand"))?;
                if !value.is_finite() {
                    return Err(malformed("nonfinite CFF real operand"));
                }
                value
            }
            _ => return Err(malformed("invalid CFF numeric operand")),
        })
    }
}

fn dictionary(
    data: &[u8],
    budget: &mut Budget<'_>,
    mut entry: impl FnMut(u16, &[f64]) -> Result<()>,
) -> Result<()> {
    let mut r = Reader { data, at: 0 };
    let mut args = [0.0; 48];
    let mut len = 0;
    let mut seen = std::collections::BTreeSet::new();
    while r.at < data.len() {
        budget.work(1)?;
        let b = r.byte()?;
        if b >= 28 {
            if len == args.len() {
                return Err(limit("CFF DICT operand stack exceeded"));
            }
            args[len] = r.number(b, true)?;
            len += 1;
        } else {
            let op = if b == 12 {
                0x0c00 | u16::from(r.byte()?)
            } else {
                u16::from(b)
            };
            if !seen.insert(op) {
                return Err(malformed("duplicate CFF DICT operator"));
            }
            if seen.len() > budget.limits.max_operations {
                return Err(limit("CFF DICT entry count exceeded"));
            }
            budget.allocate(32)?;
            entry(op, &args[..len])?;
            len = 0;
        }
    }
    if len != 0 {
        return Err(malformed("unterminated CFF DICT operands"));
    }
    Ok(())
}
fn offset(value: f64) -> Result<usize> {
    if value < 0.0 || value > u32::MAX as f64 || value.fract() != 0.0 {
        return Err(malformed("invalid CFF integer offset"));
    }
    Ok(value as usize)
}

/// Preflight only; ttf-parser remains the actual contour decoder.
pub(super) fn validate(
    data: &[u8],
    glyphs: usize,
    units: u16,
    budget: &mut Budget<'_>,
) -> Result<usize> {
    let mut r = Reader { data, at: 0 };
    if r.byte()? != 1 || r.byte()? != 0 {
        return Err(unsupported("only static CFF 1.0 is supported"));
    }
    let header = r.byte()? as usize;
    let size = r.byte()?;
    if header < 4 || !(1..=4).contains(&size) {
        return Err(malformed("invalid CFF header"));
    }
    r.take(header - 4)?;
    let names = r.index(budget)?;
    let tops = r.index(budget)?;
    if names.len() != 1 || tops.len() != 1 {
        return Err(unsupported("OpenType CFF requires one named font"));
    }
    let _strings = r.index(budget)?;
    let global = r.index(budget)?;
    let mut charstrings = None;
    let mut private = None;
    let mut matrix = [0.001, 0.0, 0.0, 0.001, 0.0, 0.0];
    dictionary(tops[0], budget, |op, args| {
        match op {
            17 if args.len() == 1 => charstrings = Some(offset(args[0])?),
            18 if args.len() == 2 => private = Some((offset(args[0])?, offset(args[1])?)),
            0x0c07 if args.len() == 6 => matrix.copy_from_slice(args),
            0x0c05 if args == [0.0] => {}
            0x0c06 if args == [2.0] => {}
            0..=5 | 13..=16 | 0x0c00..=0x0c04 | 0x0c08 => {}
            _ => {
                return Err(unsupported(
                    "CFF DICT variant, CID/synthetic font or embedded PostScript is unsupported",
                ));
            }
        }
        Ok(())
    })?;
    let expected = 1.0 / f64::from(units);
    if (matrix[0] - expected).abs() > 1e-12
        || (matrix[3] - expected).abs() > 1e-12
        || [1, 2, 4, 5].iter().any(|&i| matrix[i] != 0.0)
    {
        return Err(unsupported(
            "CFF FontMatrix must match the sfnt design-unit scale without a transform",
        ));
    }
    let mut cs = Reader {
        data,
        at: charstrings.ok_or_else(|| malformed("missing CFF CharStrings"))?,
    };
    let chars = cs.index(budget)?;
    if chars.len() != glyphs {
        return Err(malformed("CFF CharStrings count differs from maxp"));
    }
    let (len, at) = private.ok_or_else(|| malformed("missing CFF Private DICT"))?;
    let mut p = Reader { data, at };
    let private_data = p.take(len)?;
    let mut subrs = None;
    dictionary(private_data, budget, |op, args| {
        match op {
            19 if args.len() == 1 => subrs = Some(offset(args[0])?),
            6..=11 | 20..=21 | 0x0c09..=0x0c0e | 0x0c11..=0x0c13 => {}
            _ => return Err(unsupported("unsupported CFF Private DICT operator")),
        }
        Ok(())
    })?;
    let local = if let Some(relative) = subrs {
        let at = at
            .checked_add(relative)
            .ok_or_else(|| malformed("CFF subroutine offset overflow"))?;
        Reader { data, at }.index(budget)?
    } else {
        Vec::new()
    };
    let mut maximum = 0;
    for charstring in chars {
        budget.allocate(48 * std::mem::size_of::<f64>())?;
        let mut check = Program {
            local: &local,
            global: &global,
            stack: Vec::with_capacity(48),
            stems: 0,
            width: false,
            operations: 0,
            budget,
        };
        if !check.run(charstring, 0)? {
            return Err(malformed("CFF glyph lacks endchar"));
        }
        check.budget.work(check.operations as u64)?;
        maximum = maximum.max(check.operations);
    }
    Ok(maximum)
}

struct Program<'a, 'b, 'c> {
    local: &'a [&'a [u8]],
    global: &'a [&'a [u8]],
    stack: Vec<f64>,
    stems: usize,
    width: bool,
    operations: usize,
    budget: &'b mut Budget<'c>,
}
impl Program<'_, '_, '_> {
    fn run(&mut self, data: &[u8], depth: usize) -> Result<bool> {
        if depth > 10 {
            return Err(limit("CFF subroutine nesting exceeds ten"));
        }
        let mut r = Reader { data, at: 0 };
        while r.at < data.len() {
            self.operations += 1;
            if self.operations > self.budget.limits.max_operations {
                return Err(limit("CFF expanded program complexity exceeded"));
            }
            self.budget.work(1)?;
            let b = r.byte()?;
            if b == 28 || b >= 32 {
                if self.stack.len() == 48 {
                    return Err(limit("CFF Type 2 operand stack exceeded"));
                }
                self.stack.push(r.number(b, false)?);
                continue;
            }
            match b {
                10 | 29 => {
                    let operand = self
                        .stack
                        .pop()
                        .ok_or_else(|| malformed("CFF subroutine has no index"))?;
                    let table = if b == 10 { self.local } else { self.global };
                    let bias = if table.len() < 1240 {
                        107.0
                    } else if table.len() < 33900 {
                        1131.0
                    } else {
                        32768.0
                    };
                    let index = offset(operand + bias)?;
                    let sub = table
                        .get(index)
                        .ok_or_else(|| malformed("CFF subroutine index out of range"))?;
                    if self.run(sub, depth + 1)? {
                        if r.at != data.len() {
                            return Err(malformed("CFF bytes follow endchar"));
                        }
                        return Ok(true);
                    }
                }
                11 => {
                    if depth == 0 || r.at != data.len() {
                        return Err(malformed("invalid CFF return position"));
                    }
                    return Ok(false);
                }
                14 => {
                    if self.stack.len() > usize::from(!self.width) {
                        return Err(unsupported("CFF seac/composite endchar is unsupported"));
                    }
                    if r.at != data.len() {
                        return Err(malformed("CFF bytes follow endchar"));
                    }
                    return Ok(true);
                }
                1 | 3 | 18 | 23 | 19 | 20 => {
                    let len = self.stack.len();
                    if len % 2 != 0 && self.width {
                        return Err(malformed("invalid CFF hint operand count"));
                    }
                    self.stems += len / 2;
                    if self.stems > 96 {
                        return Err(limit("CFF stem count exceeded"));
                    }
                    self.width = true;
                    self.stack.clear();
                    if b == 19 || b == 20 {
                        let mask = r.take(self.stems.div_ceil(8))?;
                        if self.stems % 8 != 0
                            && mask
                                .last()
                                .is_some_and(|v| v & ((1 << (8 - self.stems % 8)) - 1) != 0)
                        {
                            return Err(malformed("nonzero unused CFF hint-mask bits"));
                        }
                    }
                }
                4 | 21 | 22 => {
                    let args = if b == 21 { 2 } else { 1 };
                    if self.stack.len() != args && (self.width || self.stack.len() != args + 1) {
                        return Err(malformed("invalid CFF moveto operands"));
                    }
                    self.width = true;
                    self.stack.clear();
                }
                5..=8 | 24..=27 | 30..=31 => self.stack.clear(),
                12 => {
                    if !matches!(r.byte()?, 34..=37) {
                        return Err(unsupported(
                            "CFF arithmetic/transient-stack operator unsupported by the outline library",
                        ));
                    }
                    self.stack.clear();
                }
                _ => return Err(malformed("invalid CFF Type 2 operator")),
            }
        }
        Err(malformed("unterminated CFF glyph or subroutine"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ErrorCode, Limits};
    fn program(data: &[u8], local: &[&[u8]], limits: &Limits) -> Result<bool> {
        let mut budget = Budget::new(limits);
        Program {
            local,
            global: &[],
            stack: Vec::new(),
            stems: 0,
            width: false,
            operations: 0,
            budget: &mut budget,
        }
        .run(data, 0)
    }
    #[test]
    fn malformed_indexes_dictionary_numbers_and_masks_fail() {
        let limits = Limits::default();
        for data in [
            &[0, 1, 0][..],
            &[0, 1, 1, 0, 1],
            &[0, 2, 1, 1, 3, 2, 0],
            &[0, 1, 4, 0, 0, 0, 1],
        ] {
            assert_eq!(
                Reader { data, at: 0 }
                    .index(&mut Budget::new(&limits))
                    .unwrap_err()
                    .code,
                ErrorCode::Malformed
            );
        }
        for data in [&[30, 0xdf][..], &[139], &[139, 17, 139, 17]] {
            assert_eq!(
                dictionary(data, &mut Budget::new(&limits), |_, _| Ok(()))
                    .unwrap_err()
                    .code,
                ErrorCode::Malformed
            );
        }
        // One stem requires a one-byte mask; its low seven bits are unused.
        for data in [&[139, 159, 1, 19][..], &[139, 159, 1, 19, 0x81, 14]] {
            assert_eq!(
                program(data, &[], &limits).unwrap_err().code,
                ErrorCode::Malformed
            );
        }
        assert!(program(&[139, 159, 1, 19, 0x80, 14], &[], &limits).unwrap());
    }
    #[test]
    fn subroutine_bounds_cycles_dynamic_operators_and_seac_are_explicit() {
        let limits = Limits::default();
        assert!(program(&[32, 10, 14], &[&[11]], &limits).unwrap());
        for data in [&[33, 10, 14][..], &[32, 10], &[11], &[14, 0]] {
            assert_eq!(
                program(data, &[&[11]], &limits).unwrap_err().code,
                ErrorCode::Malformed
            );
        }
        assert_eq!(
            program(&[32, 10, 14], &[&[32, 10, 11]], &limits)
                .unwrap_err()
                .code,
            ErrorCode::ResourceLimit
        );
        for data in [&[139, 139, 12, 10, 14][..], &[139, 139, 139, 139, 14]] {
            assert_eq!(
                program(data, &[], &limits).unwrap_err().code,
                ErrorCode::Unsupported
            );
        }
    }
    #[test]
    fn expanded_subroutine_work_is_bounded_even_without_deep_recursion() {
        let limits = Limits {
            max_operations: 10000,
            max_work: 500,
            ..Limits::default()
        };
        let mut subs = Vec::new();
        subs.push(vec![11]);
        for i in 0..8 {
            let mut data = Vec::new();
            for _ in 0..4 {
                data.extend([32 + i, 10]);
            }
            data.push(11);
            subs.push(data);
        }
        let refs = subs.iter().map(Vec::as_slice).collect::<Vec<_>>();
        assert_eq!(
            program(&[40, 10, 14], &refs, &limits).unwrap_err().code,
            ErrorCode::ResourceLimit
        );
        let limits = Limits {
            max_operations: 100,
            max_work: 10000,
            ..Limits::default()
        };
        assert_eq!(
            program(&[40, 10, 14], &refs, &limits).unwrap_err().code,
            ErrorCode::ResourceLimit
        );
    }
}
