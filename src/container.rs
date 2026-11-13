use crate::model::*;
use crate::parser::{parse_payload, signature};
use serde::Serialize;
use std::{collections::BTreeSet, ops::Range};

#[derive(Clone, Copy, Debug, Default)]
pub enum PictureSelector {
    #[default]
    Unique,
    DataFork,
    Resource(i16),
}

#[derive(Debug, Clone, Serialize)]
pub struct PictureEntry {
    pub source: &'static str,
    pub resource_id: Option<i16>,
    pub name_hex: Option<String>,
    pub offset: usize,
    pub length: usize,
}
#[derive(Debug, Serialize)]
pub struct ContainerInventory {
    pub container: Container,
    pub pictures: Vec<PictureEntry>,
}

fn bad(message: &str) -> Error {
    Error::new(ErrorCode::Malformed, message)
}
fn span(data: &[u8], start: usize, len: usize) -> Result<Range<usize>> {
    let end = start
        .checked_add(len)
        .filter(|&end| end <= data.len())
        .ok_or_else(|| bad("container range outside input"))?;
    Ok(start..end)
}
fn bytes(data: &[u8], at: usize, size: usize) -> Result<&[u8]> {
    Ok(&data[span(data, at, size)?])
}
fn u16be(data: &[u8], at: usize) -> Result<u16> {
    let b = bytes(data, at, 2)?;
    Ok(u16::from_be_bytes([b[0], b[1]]))
}
fn u32be(data: &[u8], at: usize) -> Result<usize> {
    let b = bytes(data, at, 4)?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
}
fn disjoint(ranges: &mut [Range<usize>]) -> Result<()> {
    ranges.sort_unstable_by_key(|r| r.start);
    let mut end = 0;
    for range in ranges.iter().filter(|r| r.start != r.end) {
        if range.start < end {
            return Err(bad("overlapping container ranges"));
        }
        end = range.end;
    }
    Ok(())
}
fn count(n: usize, limits: &Limits, budget: &mut Budget<'_>) -> Result<()> {
    if n > limits.max_operations {
        return Err(Error::new(
            ErrorCode::ResourceLimit,
            "container entry count limit exceeded",
        ));
    }
    budget.work(n as u64)?;
    budget.allocate(
        n.checked_mul(128)
            .ok_or_else(|| bad("entry count overflow"))?,
    )
}
fn count_minus_one(n: u16) -> usize {
    if n == 0xffff { 0 } else { n as usize + 1 }
}

fn resources(
    data: &[u8],
    base: usize,
    limits: &Limits,
    budget: &mut Budget<'_>,
    entries: &mut Vec<PictureEntry>,
) -> Result<()> {
    let data_range = span(data, u32be(data, 0)?, u32be(data, 8)?)?;
    let map_range = span(data, u32be(data, 4)?, u32be(data, 12)?)?;
    disjoint(&mut [0..16, data_range.clone(), map_range.clone()])?;
    let map = &data[map_range];
    bytes(map, 0, 28)?;
    if map[..16] != data[..16] && map[..16].iter().any(|&b| b != 0) {
        return Err(bad("resource map header copy mismatch"));
    }
    let type_at = u16be(map, 24)? as usize;
    let name_at = u16be(map, 26)? as usize;
    let types = count_minus_one(u16be(map, type_at)?);
    count(types, limits, budget)?;
    let mut layout = vec![
        0..28,
        span(map, type_at, 2 + types * 8)?,
        span(
            map,
            name_at,
            map.len()
                .checked_sub(name_at)
                .ok_or_else(|| bad("invalid resource name list"))?,
        )?,
    ];
    let mut payloads = Vec::new();
    let mut types_seen = BTreeSet::new();
    let mut total = 0usize;
    for i in 0..types {
        let entry = type_at + 2 + i * 8;
        let kind = bytes(map, entry, 4)?;
        if !types_seen.insert(kind) {
            return Err(bad("duplicate resource type"));
        }
        let n = count_minus_one(u16be(map, entry + 4)?);
        total = total
            .checked_add(n)
            .ok_or_else(|| bad("resource count overflow"))?;
        if total > limits.max_operations {
            return Err(Error::new(
                ErrorCode::ResourceLimit,
                "resource count limit exceeded",
            ));
        }
        count(n, limits, budget)?;
        let refs = type_at + u16be(map, entry + 6)? as usize;
        layout.push(span(map, refs, n * 12)?);
        let mut ids = BTreeSet::new();
        for j in 0..n {
            let at = refs + j * 12;
            let id = u16be(map, at)? as i16;
            if !ids.insert(id) {
                return Err(bad("duplicate resource ID within type"));
            }
            let name = u16be(map, at + 2)?;
            let name_hex = if name == 0xffff {
                None
            } else {
                let at = name_at + name as usize;
                let len = *bytes(map, at, 1)?.first().expect("one byte") as usize;
                let value = bytes(map, at + 1, len)?;
                use std::fmt::Write;
                let mut hex = String::new();
                for b in value {
                    write!(hex, "{b:02x}").unwrap();
                }
                budget.allocate(hex.len())?;
                Some(hex)
            };
            let off = bytes(map, at + 5, 3)?;
            let off = ((off[0] as usize) << 16) | ((off[1] as usize) << 8) | off[2] as usize;
            let payload = data_range
                .start
                .checked_add(off)
                .ok_or_else(|| bad("resource offset overflow"))?;
            let len = u32be(data, payload)?;
            let range = span(
                data,
                payload,
                4usize
                    .checked_add(len)
                    .ok_or_else(|| bad("resource length overflow"))?,
            )?;
            if range.start < data_range.start || range.end > data_range.end {
                return Err(bad("resource outside data area"));
            }
            payloads.push(range);
            if kind == b"PICT" {
                entries.push(PictureEntry {
                    source: "resource",
                    resource_id: Some(id),
                    name_hex,
                    offset: base + payload + 4,
                    length: len,
                });
            }
        }
    }
    layout.retain(|r| r.start != r.end);
    disjoint(&mut layout)?;
    disjoint(&mut payloads)?;
    Ok(())
}

fn data_fork(data: &[u8], base: usize, entries: &mut Vec<PictureEntry>) -> Result<()> {
    match (signature(data, 0), signature(data, 512)) {
        (true, true) => return Err(bad("ambiguous raw/headered picture in data fork")),
        (true, false) => entries.push(PictureEntry {
            source: "data_fork",
            resource_id: None,
            name_hex: None,
            offset: base,
            length: data.len(),
        }),
        (false, true) => entries.push(PictureEntry {
            source: "data_fork",
            resource_id: None,
            name_hex: None,
            offset: base + 512,
            length: data.len() - 512,
        }),
        _ => {}
    }
    Ok(())
}
fn aligned(n: usize) -> Result<usize> {
    n.checked_add(127)
        .map(|n| n & !127)
        .ok_or_else(|| bad("MacBinary padding overflow"))
}
fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for b in data {
        crc ^= (*b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}
fn mac_signature(data: &[u8]) -> bool {
    data.len() >= 128
        && data[0] == 0
        && (1..=63).contains(&data[1])
        && data[2..2 + data[1] as usize].iter().all(|&b| b != 0)
        && data[74] == 0
        && data[82] == 0
}
fn macbinary(
    data: &[u8],
    limits: &Limits,
    budget: &mut Budget<'_>,
    entries: &mut Vec<PictureEntry>,
) -> Result<()> {
    if !mac_signature(data) {
        return Err(bad("invalid MacBinary header"));
    }
    match data[122] {
        0 => {
            if data[101..126].iter().any(|&b| b != 0) {
                return Err(bad("invalid MacBinary I reserved header"));
            }
        }
        129 | 130 => {
            if data[123] > 130 {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "MacBinary minimum reader version unsupported",
                ));
            }
            if crc16(&data[..124]) != u16be(data, 124)? {
                return Err(bad("MacBinary header CRC mismatch"));
            }
            if data[122] == 130 && &data[102..106] != b"mBIN" {
                return Err(bad("invalid MacBinary III signature"));
            }
            if u32be(data, 116)? != 0 {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "packed MacBinary is not implemented",
                ));
            }
        }
        _ => {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "MacBinary version not implemented",
            ));
        }
    }
    let start = 128 + aligned(u16be(data, 120)? as usize)?;
    let dl = u32be(data, 83)?;
    let rl = u32be(data, 87)?;
    let dr = span(data, start, dl)?;
    let rs = start
        .checked_add(aligned(dl)?)
        .ok_or_else(|| bad("MacBinary fork offset overflow"))?;
    let rr = span(data, rs, rl)?;
    let end = rs
        .checked_add(aligned(rl)?)
        .ok_or_else(|| bad("MacBinary fork offset overflow"))?;
    let comment = u16be(data, 99)? as usize;
    let cr = span(data, end, comment)?;
    if cr.end != data.len() {
        return Err(bad("unexpected trailing or missing MacBinary padding"));
    }
    for range in [
        span(data, dr.end, rs - dr.end)?,
        span(data, rr.end, end - rr.end)?,
    ] {
        if data[range].iter().any(|&b| b != 0) {
            return Err(bad("nonzero MacBinary fork padding"));
        }
    }
    if dl > 0 {
        data_fork(&data[dr], start, entries)?;
    }
    if rl > 0 {
        resources(&data[rr], rs, limits, budget, entries)?;
    }
    Ok(())
}
fn appledouble(
    data: &[u8],
    limits: &Limits,
    budget: &mut Budget<'_>,
    entries: &mut Vec<PictureEntry>,
) -> Result<()> {
    if u32be(data, 0)? != 0x00051607 {
        return Err(bad("invalid AppleDouble magic"));
    }
    if u32be(data, 4)? != 0x00020000 {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "only AppleDouble version 2 is supported",
        ));
    }
    let n = u16be(data, 24)? as usize;
    count(n, limits, budget)?;
    let mut layout = vec![span(data, 0, 26 + n * 12)?];
    let mut ids = BTreeSet::new();
    for i in 0..n {
        let at = 26 + i * 12;
        let id = u32be(data, at)?;
        if id == 0 || !ids.insert(id) {
            return Err(bad("invalid or duplicate AppleDouble entry ID"));
        }
        if id == 1 {
            return Err(bad("AppleDouble must not contain a data fork entry"));
        }
        let range = span(data, u32be(data, at + 4)?, u32be(data, at + 8)?)?;
        if range.start == range.end {
            continue;
        }
        layout.push(range.clone());
        if id == 2 {
            resources(&data[range.clone()], range.start, limits, budget, entries)?;
        }
    }
    disjoint(&mut layout)
}
fn looks_like_resources(data: &[u8]) -> bool {
    if data.len() < 16 {
        return false;
    }
    let get = |at| u32be(data, at).expect("header available");
    get(0) >= 16
        && get(4) >= 16
        && get(12) >= 28
        && span(data, get(0), get(8)).is_ok()
        && span(data, get(4), get(12)).is_ok()
}

/// Inspect container ranges without decoding pictures or extracting files to disk.
pub fn list_pictures(
    data: &[u8],
    container: Container,
    limits: &Limits,
) -> Result<ContainerInventory> {
    limits.validate()?;
    if data.len() > limits.max_input_bytes {
        return Err(Error::new(
            ErrorCode::ResourceLimit,
            "input byte limit exceeded",
        ));
    }
    let mut budget = Budget::new(limits);
    budget.work(data.len() as u64)?;
    let container = if container == Container::Auto {
        let candidates: Vec<_> = [
            (signature(data, 0), Container::Raw),
            (signature(data, 512), Container::File),
            (looks_like_resources(data), Container::ResourceFork),
            (mac_signature(data), Container::MacBinary),
            (data.starts_with(&[0, 5, 0x16, 7]), Container::AppleDouble),
        ]
        .into_iter()
        .filter_map(|(yes, kind)| yes.then_some(kind))
        .collect();
        if candidates.len() != 1 {
            return Err(bad(
                "missing or ambiguous container signature; select --container explicitly",
            ));
        }
        candidates[0]
    } else {
        container
    };
    let mut pictures = Vec::new();
    match container {
        Container::Raw | Container::File => {
            let offset = if container == Container::File { 512 } else { 0 };
            let len = data
                .len()
                .checked_sub(offset)
                .ok_or_else(|| bad("missing PICT file header"))?;
            pictures.push(PictureEntry {
                source: "data_fork",
                resource_id: None,
                name_hex: None,
                offset,
                length: len,
            });
        }
        Container::ResourceFork => resources(data, 0, limits, &mut budget, &mut pictures)?,
        Container::MacBinary => macbinary(data, limits, &mut budget, &mut pictures)?,
        Container::AppleDouble => appledouble(data, limits, &mut budget, &mut pictures)?,
        Container::Auto => unreachable!(),
    }
    Ok(ContainerInventory {
        container,
        pictures,
    })
}

/// Select a unique picture, a data-fork picture, or an exact signed PICT resource ID.
pub fn parse_selected(
    data: &[u8],
    container: Container,
    selector: PictureSelector,
    limits: &Limits,
) -> Result<Picture> {
    let inventory = list_pictures(data, container, limits)?;
    let choices: Vec<_> = inventory
        .pictures
        .iter()
        .filter(|p| match selector {
            PictureSelector::Unique => true,
            PictureSelector::DataFork => p.source == "data_fork",
            PictureSelector::Resource(id) => p.resource_id == Some(id),
        })
        .collect();
    if choices.len() != 1 {
        return Err(Error::new(
            ErrorCode::InvalidArguments,
            format!(
                "picture selection matched {} entries; list-pictures and select --resource-id or --data-fork",
                choices.len()
            ),
        ));
    }
    let entry = choices[0];
    let mut picture = parse_payload(
        &data[span(data, entry.offset, entry.length)?],
        entry.offset,
        inventory.container,
        limits,
    )?;
    picture.resource_id = entry.resource_id;
    Ok(picture)
}
