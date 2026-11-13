use crate::model::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrinterComments {
    #[default]
    Ignore,
    ApplePortable,
}

fn unsupported(message: &str) -> Error {
    Error::new(ErrorCode::Unsupported, message)
}
fn malformed(message: &str) -> Error {
    Error::new(ErrorCode::Malformed, message)
}

#[derive(Default)]
struct Rotation {
    angle: f64,
    flip: u8,
    offset: Option<[f64; 2]>,
    center: Option<[f64; 2]>,
}

pub(crate) struct PrinterState {
    rotation: Option<Rotation>,
    flush: bool,
    factor: [u64; 2],
}
impl Default for PrinterState {
    fn default() -> Self {
        Self {
            rotation: None,
            flush: false,
            factor: [1, 1],
        }
    }
}
impl PrinterState {
    pub fn rotating(&self) -> bool {
        self.rotation.is_some()
    }
    pub fn finish(&self) -> Result<()> {
        if self.rotation.is_some() || self.flush {
            return Err(malformed("unterminated printer comment group"));
        }
        Ok(())
    }
    pub fn intercept(&mut self, command: &Command) -> Result<bool> {
        if self.flush && !matches!(command, Command::Comment { kind: 191, .. }) {
            return Err(unsupported(
                "only empty PostScriptBegin/PostScriptEnd synchronization pairs are supported; no PostScript is executed",
            ));
        }
        if let Command::Comment { kind, data } = command {
            let empty = || {
                if data.is_empty() {
                    Ok(())
                } else {
                    Err(malformed("printer delimiter requires empty data"))
                }
            };
            match kind {
                150 => {
                    if self.rotation.is_some() {
                        return Err(malformed("nested TextBegin"));
                    }
                    if !matches!(data.len(), 6 | 10) {
                        return Err(malformed("TextBegin requires 6 or 10 bytes"));
                    }
                    if data[0] != 0 || data[1] > 2 || data[4..6] != [0, 0] {
                        return Err(unsupported(
                            "portable TextBegin requires no alignment, known flip and zero reserved fields",
                        ));
                    }
                    let angle = if data.len() == 10 {
                        f64::from(i32::from_be_bytes(data[6..10].try_into().unwrap())) / 65536.0
                    } else {
                        f64::from(i16::from_be_bytes(data[2..4].try_into().unwrap()))
                    };
                    if !(-360.0..=360.0).contains(&angle) {
                        return Err(unsupported(
                            "TextBegin angle must be within -360..360 degrees",
                        ));
                    }
                    if data[1] != 0 && angle.rem_euclid(360.0) != 0.0 {
                        return Err(unsupported(
                            "combined flip/rotation order is outside the portable printer profile",
                        ));
                    }
                    self.rotation = Some(Rotation {
                        angle,
                        flip: data[1],
                        ..Default::default()
                    });
                }
                151 => {
                    empty()?;
                    if self.rotation.take().is_none() {
                        return Err(malformed("TextEnd without TextBegin"));
                    }
                }
                154 => {
                    if data.len() != 8 {
                        return Err(malformed("TextCenter requires two Fixed offsets"));
                    }
                    let rotation = self
                        .rotation
                        .as_mut()
                        .ok_or_else(|| malformed("TextCenter without TextBegin"))?;
                    let fixed = |at| {
                        f64::from(i32::from_be_bytes(data[at..at + 4].try_into().unwrap()))
                            / 65536.0
                    };
                    rotation.offset = Some([fixed(4), fixed(0)]);
                    rotation.center = None;
                }
                182 => {
                    if data.len() != 4 {
                        return Err(malformed("SetLineWidth requires numerator/denominator"));
                    }
                    let n = i16::from_be_bytes(data[..2].try_into().unwrap());
                    let d = i16::from_be_bytes(data[2..].try_into().unwrap());
                    if n <= 0 || d <= 0 {
                        return Err(unsupported("SetLineWidth requires a positive finite ratio"));
                    }
                    let gcd = |mut a: u64, mut b: u64| {
                        while b != 0 {
                            (a, b) = (b, a % b);
                        }
                        a
                    };
                    let [a, b] = self.factor;
                    let (n, d) = (n as u64, d as u64);
                    let (g1, g2) = (gcd(a, d), gcd(n, b));
                    let mul = |x: u64, y: u64| {
                        x.checked_mul(y).ok_or_else(|| {
                            Error::new(ErrorCode::ResourceLimit, "printer width ratio overflow")
                        })
                    };
                    let (n, d) = (mul(a / g1, n / g2)?, mul(b / g2, d / g1)?);
                    let g = gcd(n, d);
                    self.factor = [n / g, d / g];
                    if u128::from(n) > u128::from(d) * 32768
                        || u128::from(n) * 65536 < u128::from(d)
                    {
                        return Err(Error::new(
                            ErrorCode::ResourceLimit,
                            "printer width scale outside bounded profile",
                        ));
                    }
                }
                190 => {
                    empty()?;
                    self.flush = true;
                }
                191 => {
                    empty()?;
                    if !self.flush {
                        return Err(malformed("PostScriptEnd without PostScriptBegin"));
                    }
                    self.flush = false;
                }
                _ => {
                    return Err(unsupported(
                        "picture comment is outside the explicit portable printer profile",
                    ));
                }
            }
            return Ok(true);
        }
        if self.rotating() {
            match command {
                Command::Bitmap(bitmap) if !bitmap.raw_copy => return Ok(true),
                Command::Origin(_) => {
                    return Err(unsupported(
                        "origin changes inside TextBegin require a separate coordinate-lifetime contract; move the origin outside the group",
                    ));
                }
                Command::Line(..)
                | Command::LineFrom(_)
                | Command::ShortLineFrom(_)
                | Command::Draw(..)
                | Command::Bitmap(_) => {
                    return Err(unsupported(
                        "nontext drawing inside TextBegin is outside the portable rotation profile",
                    ));
                }
                _ => {}
            }
        }
        if self.factor != [1, 1] && matches!(command, Command::Draw(Verb::Frame, _)) {
            return Err(unsupported(
                "fractional printer widths currently support axis-aligned lines, not shape frames",
            ));
        }
        Ok(false)
    }
    pub fn line(&self, from: Point, to: Point, pen: Point) -> Result<Option<[f64; 4]>> {
        if self.factor == [1, 1] || pen.x == 0 || pen.y == 0 {
            return Ok(None);
        }
        if from.x != to.x && from.y != to.y {
            return Err(unsupported(
                "fractional printer widths currently require axis-aligned lines",
            ));
        }
        let factor = self.factor[0] as f64 / self.factor[1] as f64;
        Ok(Some([
            f64::from(from.x.min(to.x)),
            f64::from(from.y.min(to.y)),
            f64::from(from.x.max(to.x)) + f64::from(pen.x) * factor,
            f64::from(from.y.max(to.y)) + f64::from(pen.y) * factor,
        ]))
    }
    pub fn transform(&mut self, baseline: Point) -> Result<Option<Transform>> {
        let Some(rotation) = self.rotation.as_mut() else {
            return Ok(None);
        };
        let offset = rotation
            .offset
            .ok_or_else(|| malformed("rotated text requires TextCenter"))?;
        let center = *rotation.center.get_or_insert([
            f64::from(baseline.x) + offset[0],
            f64::from(baseline.y) + offset[1],
        ]);
        let (sin, cos) = match rotation.angle.rem_euclid(360.0) {
            0.0 => (0.0, 1.0),
            90.0 => (1.0, 0.0),
            180.0 => (0.0, -1.0),
            270.0 => (-1.0, 0.0),
            angle => angle.to_radians().sin_cos(),
        };
        Ok(Some(Transform {
            center,
            sin,
            cos,
            flip: rotation.flip,
        }))
    }
}

pub(crate) struct Transform {
    center: [f64; 2],
    sin: f64,
    cos: f64,
    flip: u8,
}
impl Transform {
    pub fn point(&self, p: [f64; 2]) -> [f64; 2] {
        let x = (p[0] - self.center[0]) * if self.flip == 1 { -1.0 } else { 1.0 };
        let y = (p[1] - self.center[1]) * if self.flip == 2 { -1.0 } else { 1.0 };
        [
            self.center[0] + self.cos * x - self.sin * y,
            self.center[1] + self.sin * x + self.cos * y,
        ]
    }
}

pub(crate) fn bounds(r: [f64; 4]) -> Result<Rect> {
    let r = [r[0].floor(), r[1].floor(), r[2].ceil(), r[3].ceil()];
    if r.iter()
        .any(|v| !v.is_finite() || *v < i32::MIN as f64 || *v > i32::MAX as f64)
    {
        return Err(Error::new(
            ErrorCode::ResourceLimit,
            "printer coordinate overflow",
        ));
    }
    Ok(Rect {
        left: r[0] as i32,
        top: r[1] as i32,
        right: r[2] as i32,
        bottom: r[3] as i32,
    })
}

pub(crate) fn policy(
    options: &crate::RenderOptions,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<()> {
    if options.printer_comments == PrinterComments::ApplePortable {
        if options.fidelity != Fidelity::Approximate {
            return Err(unsupported(
                "apple-portable printer comments require --fidelity approximate",
            ));
        }
        diagnostics.push(Diagnostic::new(
            "portable_printer_comments",
            "Explicit Apple printer-comment subset: no classic-screen/driver equivalence, font substitution or PostScript execution; raster uses pixel-center coverage before final resampling.",
            None,
        ));
    }
    Ok(())
}
