use super::{Decoded, Palette, RleReader, direct_pixel, malformed, unsupported};
use crate::model::{Budget, Error, ErrorCode, Result};

type Block = [[u8; 3]; 16];

fn rgb555(value: u16) -> [u8; 3] {
    direct_pixel(&value.to_be_bytes(), 16, false)
        .expect("RGB555 without reserved-bit restriction")
        .0
}

fn quad(a: u16, b: u16) -> [[u8; 3]; 4] {
    let mut colors = [rgb555(b), [0; 3], [0; 3], rgb555(a)];
    for (channel, shift) in [10, 5, 0].into_iter().enumerate() {
        let (a, b) = ((a >> shift) & 31, (b >> shift) & 31);
        for (slot, wa, wb) in [(1, 11, 21), (2, 21, 11)] {
            let v = ((wa * a + wb * b) / 32) as u8;
            colors[slot][channel] = (v << 3) | (v >> 2);
        }
    }
    colors
}

fn indexed_quad(r: &mut RleReader<'_>, colors: [[u8; 3]; 4]) -> Result<Block> {
    let mut block = [[0; 3]; 16];
    for (row, &flags) in r.take(4)?.iter().enumerate() {
        for x in 0..4 {
            block[row * 4 + x] = colors[usize::from((flags >> (6 - 2 * x)) & 3)];
        }
    }
    Ok(block)
}

fn run_end(done: usize, count: usize, total: usize) -> Result<usize> {
    done.checked_add(count)
        .filter(|&n| n <= total)
        .ok_or_else(|| malformed("block codec run exceeds image"))
}

fn rpza(r: &mut RleReader<'_>, blocks: &mut Vec<Block>, total: usize) -> Result<()> {
    let opcode = r.byte()?;
    if opcode & 0x80 == 0 {
        let a = u16::from(opcode) << 8 | u16::from(r.byte()?);
        let b = r.word()?;
        let block = if b & 0x8000 != 0 {
            indexed_quad(r, quad(a, b))?
        } else {
            let mut block = [[0; 3]; 16];
            block[0] = rgb555(a);
            block[1] = rgb555(b);
            for pixel in &mut block[2..] {
                *pixel = rgb555(r.word()?);
            }
            block
        };
        blocks.push(block);
        return Ok(());
    }
    let end = run_end(blocks.len(), usize::from(opcode & 31) + 1, total)?;
    match opcode & 0xe0 {
        0x80 => {
            return Err(unsupported(
                "RPZA skipped blocks require an unavailable previous frame",
            ));
        }
        0xa0 => {
            let block = [rgb555(r.word()?); 16];
            blocks.resize(end, block);
        }
        0xc0 => {
            let colors = quad(r.word()?, r.word()?);
            while blocks.len() < end {
                blocks.push(indexed_quad(r, colors)?);
            }
        }
        _ => return Err(unsupported("unknown RPZA opcode")),
    }
    Ok(())
}

struct Caches {
    slots: [[Option<[u8; 8]>; 256]; 3],
    next: [usize; 3],
}

fn smc_indices(r: &mut RleReader<'_>, bits: usize) -> Result<[usize; 16]> {
    let mut indices = [0; 16];
    if bits == 3 {
        // SMC octets interleave nibbles from the two groups of eight selectors.
        let bytes = r.take(6)?;
        let nibbles: [u8; 12] =
            std::array::from_fn(|i| (bytes[i / 2] >> if i % 2 == 0 { 4 } else { 0 }) & 15);
        for (half, order) in [[0, 1, 2, 4, 5, 6], [8, 9, 10, 3, 7, 11]]
            .iter()
            .enumerate()
        {
            let flags = order
                .iter()
                .fold(0u32, |v, &n| (v << 4) | u32::from(nibbles[n]));
            for i in 0..8 {
                indices[half * 8 + i] = ((flags >> (21 - i * 3)) & 7) as usize;
            }
        }
    } else {
        let bytes = r.take(bits * 2)?;
        for (i, value) in indices.iter_mut().enumerate() {
            *value = usize::from(
                (bytes[i * bits / 8] >> (8 - bits - (i * bits % 8))) & ((1 << bits) - 1),
            );
        }
    }
    Ok(indices)
}

fn palette_color(palette: &Palette, index: u8) -> Result<[u8; 3]> {
    palette[usize::from(index)]
        .ok_or_else(|| malformed("SMC pixel references absent color table entry"))
}

fn smc(
    r: &mut RleReader<'_>,
    blocks: &mut Vec<Block>,
    total: usize,
    palette: &Palette,
    caches: &mut Caches,
) -> Result<()> {
    let opcode = r.byte()?;
    let class = opcode >> 4;
    let count = if class < 8 && class & 1 != 0 {
        usize::from(r.byte()?) + 1
    } else {
        usize::from(opcode & 15) + 1
    };
    let count = if matches!(class, 4 | 5) {
        count * 2
    } else {
        count
    };
    let end = run_end(blocks.len(), count, total)?;
    match class {
        0 | 1 => {
            return Err(unsupported(
                "SMC skipped blocks require an unavailable previous frame",
            ));
        }
        2..=5 => {
            let distance = if class < 4 { 1 } else { 2 };
            if blocks.len() < distance {
                return Err(unsupported(
                    "SMC repeated blocks are not available in this still",
                ));
            }
            while blocks.len() < end {
                blocks.push(blocks[blocks.len() - distance]);
            }
        }
        6 | 7 => {
            let block = [palette_color(palette, r.byte()?)?; 16];
            blocks.resize(end, block);
        }
        8..=13 => {
            let family = usize::from((class - 8) / 2);
            let bits = family + 1;
            let colors = if class & 1 == 0 {
                let mut colors = [0; 8];
                colors[..1 << bits].copy_from_slice(r.take(1 << bits)?);
                let slot = caches.next[family];
                caches.slots[family][slot] = Some(colors);
                caches.next[family] = (slot + 1) % 256;
                colors
            } else {
                caches.slots[family][usize::from(r.byte()?)].ok_or_else(|| {
                    unsupported("SMC color cache entry was not initialized by this still")
                })?
            };
            while blocks.len() < end {
                let mut block = [[0; 3]; 16];
                for (pixel, index) in block.iter_mut().zip(smc_indices(r, bits)?) {
                    *pixel = palette_color(palette, colors[index])?;
                }
                blocks.push(block);
            }
        }
        14 => {
            while blocks.len() < end {
                let mut block = [[0; 3]; 16];
                for (pixel, &index) in block.iter_mut().zip(r.take(16)?) {
                    *pixel = palette_color(palette, index)?;
                }
                blocks.push(block);
            }
        }
        _ => return Err(unsupported("unknown SMC opcode")),
    }
    Ok(())
}

pub(super) fn decode(
    codec: &[u8],
    bytes: &[u8],
    width: usize,
    height: usize,
    palette: Option<&Palette>,
    budget: &mut Budget<'_>,
) -> Result<Decoded> {
    let is_smc = codec == b"smc ";
    let mut r = RleReader {
        bytes,
        at: 0,
        label: if is_smc { "SMC" } else { "RPZA" },
    };
    let header = r.take(4)?;
    if !(if is_smc {
        matches!(header[0], 0 | 0x80 | 0xe1)
    } else {
        header[0] == 0xe1
    }) {
        return Err(unsupported("unsupported block codec chunk marker"));
    }
    let size =
        (usize::from(header[1]) << 16) | (usize::from(header[2]) << 8) | usize::from(header[3]);
    if size != bytes.len() {
        return Err(malformed("block codec chunk length mismatch"));
    }
    let columns = width.div_ceil(4);
    let total = columns
        .checked_mul(height.div_ceil(4))
        .ok_or_else(|| Error::new(ErrorCode::ResourceLimit, "block count overflow"))?;
    budget.allocate(
        total
            .checked_mul(std::mem::size_of::<Block>())
            .ok_or_else(|| Error::new(ErrorCode::ResourceLimit, "block storage overflow"))?,
    )?;
    budget.work(total as u64 * 16)?;
    let mut blocks = Vec::with_capacity(total);
    let mut caches = if is_smc {
        budget.allocate(std::mem::size_of::<Caches>())?;
        Some(Caches {
            slots: [[None; 256]; 3],
            next: [0; 3],
        })
    } else {
        None
    };
    let mut commands = 0;
    while blocks.len() < total {
        if r.at == bytes.len() {
            return Err(unsupported(
                "incomplete still leaves blocks dependent on a previous frame",
            ));
        }
        commands += 1;
        if commands > budget.limits.max_operations {
            return Err(Error::new(
                ErrorCode::ResourceLimit,
                "block codec command count limit exceeded",
            ));
        }
        budget.work(1)?;
        if let Some(caches) = &mut caches {
            smc(
                &mut r,
                &mut blocks,
                total,
                palette
                    .ok_or_else(|| unsupported("SMC requires an explicit embedded color table"))?,
                caches,
            )?;
        } else {
            rpza(&mut r, &mut blocks, total)?;
        }
    }
    if r.at != bytes.len() {
        return Err(malformed("trailing data after block codec still"));
    }
    budget.allocate(width * height * 3)?;
    let mut rgb = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        for x in 0..width {
            rgb.extend_from_slice(&blocks[(y / 4) * columns + x / 4][(y % 4) * 4 + x % 4]);
        }
    }
    Ok(Decoded { rgb, alpha: None })
}
