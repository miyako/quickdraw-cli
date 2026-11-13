#![allow(dead_code)]

pub fn word(value: i16) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}
pub fn words(values: &[i16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}
pub fn rect(top: i16, left: i16, bottom: i16, right: i16) -> Vec<u8> {
    words(&[top, left, bottom, right])
}

/// Original synthetic fixture builder; no files or expected pixels copied from other projects.
pub struct Pict {
    pub bytes: Vec<u8>,
    pub version: u8,
}
impl Pict {
    pub fn new(version: u8, width: i16, height: i16) -> Self {
        Self::framed(version, [0, 0, height, width])
    }
    pub fn framed(version: u8, frame: [i16; 4]) -> Self {
        let mut bytes = vec![0, 0];
        bytes.extend(words(&frame));
        if version == 1 {
            bytes.extend([0x11, 1]);
        } else {
            bytes.extend([0, 0x11, 2, 0xff, 0x0c, 0]);
            bytes.extend([0xff; 4]);
            bytes.extend([0; 20]);
        }
        Self { bytes, version }
    }
    pub fn op(&mut self, opcode: u16, data: &[u8]) -> &mut Self {
        if self.version == 2 {
            if self.bytes.len() % 2 == 1 {
                self.bytes.push(0);
            }
            self.bytes.extend(opcode.to_be_bytes());
        } else {
            self.bytes.push(opcode as u8);
        }
        self.bytes.extend_from_slice(data);
        self
    }
    pub fn finish(mut self) -> Vec<u8> {
        self.op(0xff, &[]);
        if self.version == 1 {
            let size = (self.bytes.len() as u16).to_be_bytes();
            self.bytes[..2].copy_from_slice(&size);
        }
        self.bytes
    }
}

pub fn region(bounds: [i16; 4], rows: &[(i16, &[i16])]) -> Vec<u8> {
    let mut result = vec![0, 0];
    result.extend(words(&bounds));
    if !rows.is_empty() {
        for (y, xs) in rows {
            result.extend(word(*y));
            result.extend(words(xs));
            result.extend(word(0x7fff));
        }
        result.extend(word(0x7fff));
    }
    let size = (result.len() as u16).to_be_bytes();
    result[..2].copy_from_slice(&size);
    result
}

pub fn mono(row_bytes: i16, bounds: [i16; 4], dest: [i16; 4], mode: i16, data: &[u8]) -> Vec<u8> {
    let mut result = word(row_bytes);
    result.extend(words(&bounds));
    result.extend(words(&bounds));
    result.extend(words(&dest));
    result.extend(word(mode));
    result.extend(data);
    result
}

pub fn pixmap_header(
    row_bytes: u16,
    bounds: [i16; 4],
    depth: u16,
    pack: u16,
    direct: bool,
    components: u16,
) -> Vec<u8> {
    let mut data = Vec::new();
    if direct {
        data.extend(0xffu32.to_be_bytes());
    }
    data.extend((row_bytes | 0x8000).to_be_bytes());
    data.extend(words(&bounds));
    data.extend(0u16.to_be_bytes());
    data.extend(pack.to_be_bytes());
    data.extend(0u32.to_be_bytes());
    data.extend((72u32 << 16).to_be_bytes());
    data.extend((72u32 << 16).to_be_bytes());
    data.extend((if direct { 16u16 } else { 0 }).to_be_bytes());
    data.extend(depth.to_be_bytes());
    data.extend(components.to_be_bytes());
    data.extend(
        (if direct {
            if depth == 16 { 5u16 } else { 8 }
        } else {
            depth
        })
        .to_be_bytes(),
    );
    data.extend([0; 12]);
    data
}

pub fn direct(depth: u16, pack: u16, width: i16, row_bytes: u16, encoded: &[u8]) -> Vec<u8> {
    let bounds = [0, 0, 1, width];
    let mut result = pixmap_header(row_bytes, bounds, depth, pack, true, 3);
    result.extend(words(&bounds));
    result.extend(words(&bounds));
    result.extend(word(0));
    result.extend(encoded);
    result
}

pub fn nfnt() -> Vec<u8> {
    let mut bytes = words(&[0x1000, 65, 66, 4, 0, 0, 3, 3, 12, 3, 0, 0, 1]);
    bytes.extend([0x5c, 0, 0xb4, 0, 0xfc, 0]); // authored A, B, missing glyph
    bytes.extend(words(&[0, 3, 5, 6])); // A, B, missing, end
    bytes.extend(words(&[4, 0x0103, 2, -1])); // bearings/advances and terminator
    bytes
}

pub fn resource_fork(records: &[(i16, Vec<u8>)]) -> Vec<u8> {
    let mut payload = Vec::new();
    let mut offsets = Vec::new();
    for (_, bytes) in records {
        offsets.push(payload.len());
        payload.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        payload.extend_from_slice(bytes);
    }
    let mut map = vec![0; 38 + records.len() * 12 + 5];
    map[24..26].copy_from_slice(&28u16.to_be_bytes());
    map[26..28].copy_from_slice(&((38 + records.len() * 12) as u16).to_be_bytes());
    map[30..34].copy_from_slice(b"PICT");
    map[34..36].copy_from_slice(&(records.len() as u16).wrapping_sub(1).to_be_bytes());
    map[36..38].copy_from_slice(&10u16.to_be_bytes());
    for (i, (id, _)) in records.iter().enumerate() {
        let at = 38 + i * 12;
        map[at..at + 2].copy_from_slice(&id.to_be_bytes());
        map[at + 5..at + 8].copy_from_slice(&(offsets[i] as u32).to_be_bytes()[1..]);
    }
    let names = 38 + records.len() * 12;
    map[names..].copy_from_slice(b"\x04test");
    let mut header = Vec::new();
    for n in [256, 256 + payload.len(), payload.len(), map.len()] {
        header.extend_from_slice(&(n as u32).to_be_bytes());
    }
    map[..16].copy_from_slice(&header);
    header.resize(256, 0);
    header.extend(payload);
    header.extend(map);
    header
}
pub fn macbinary(data: &[u8], resource: &[u8], version: u8) -> Vec<u8> {
    let mut bytes = vec![0; 128];
    bytes[1..6].copy_from_slice(b"\x04test");
    bytes[65..69].copy_from_slice(b"PICT");
    bytes[83..87].copy_from_slice(&(data.len() as u32).to_be_bytes());
    bytes[87..91].copy_from_slice(&(resource.len() as u32).to_be_bytes());
    if version != 0 {
        bytes[122] = version;
        bytes[123] = 129;
        if version == 130 {
            bytes[102..106].copy_from_slice(b"mBIN");
        }
        let mut crc = 0u16;
        for b in &bytes[..124] {
            crc ^= (*b as u16) << 8;
            for _ in 0..8 {
                crc = if crc & 0x8000 != 0 {
                    (crc << 1) ^ 0x1021
                } else {
                    crc << 1
                };
            }
        }
        bytes[124..126].copy_from_slice(&crc.to_be_bytes());
    }
    bytes.extend_from_slice(data);
    bytes.resize(bytes.len().next_multiple_of(128), 0);
    bytes.extend_from_slice(resource);
    bytes.resize(bytes.len().next_multiple_of(128), 0);
    bytes
}
pub fn appledouble(resource: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0, 5, 0x16, 7, 0, 2, 0, 0];
    bytes.extend([0; 16]);
    bytes.extend(word(1));
    for n in [2, 38, resource.len()] {
        bytes.extend_from_slice(&(n as u32).to_be_bytes());
    }
    bytes.extend_from_slice(resource);
    bytes
}

pub fn applesingle(data: &[u8], resource: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0, 5, 0x16, 0, 0, 2, 0, 0];
    bytes.extend([0; 16]);
    bytes.extend(word(2));
    for n in [1, 50, data.len(), 2, 50 + data.len(), resource.len()] {
        bytes.extend_from_slice(&(n as u32).to_be_bytes());
    }
    bytes.extend_from_slice(data);
    bytes.extend_from_slice(resource);
    bytes
}

pub fn font_resources(records: &[([u8; 4], i16, Vec<u8>)]) -> Vec<u8> {
    let mut groups = std::collections::BTreeMap::<_, Vec<_>>::new();
    let mut payload = Vec::new();
    for (kind, id, bytes) in records {
        groups.entry(*kind).or_default().push((*id, payload.len()));
        payload.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        payload.extend_from_slice(bytes);
    }
    let refs = 30 + groups.len() * 8;
    let names = refs + records.len() * 12;
    let mut map = vec![0; names];
    map[24..26].copy_from_slice(&28u16.to_be_bytes());
    map[26..28].copy_from_slice(&(names as u16).to_be_bytes());
    map[28..30].copy_from_slice(&(groups.len() as u16).wrapping_sub(1).to_be_bytes());
    let mut reference = refs;
    for (index, (kind, entries)) in groups.iter().enumerate() {
        let at = 30 + index * 8;
        map[at..at + 4].copy_from_slice(kind);
        map[at + 4..at + 6].copy_from_slice(&(entries.len() as u16 - 1).to_be_bytes());
        map[at + 6..at + 8].copy_from_slice(&((reference - 28) as u16).to_be_bytes());
        for (id, offset) in entries {
            map[reference..reference + 2].copy_from_slice(&id.to_be_bytes());
            map[reference + 2..reference + 4].copy_from_slice(&0xffffu16.to_be_bytes());
            map[reference + 5..reference + 8].copy_from_slice(&(*offset as u32).to_be_bytes()[1..]);
            reference += 12;
        }
    }
    let mut header = Vec::new();
    for n in [256, 256 + payload.len(), payload.len(), map.len()] {
        header.extend_from_slice(&(n as u32).to_be_bytes());
    }
    map[..16].copy_from_slice(&header);
    header.resize(256, 0);
    header.extend(payload);
    header.extend(map);
    header
}

// Original 52-byte FamRec, one intrinsic strike and Adobe TN 0091 width/kern tables.
pub fn fond() -> Vec<u8> {
    let mut bytes = words(&[2, 7, 65, 66, 4096, 0, 0, 4096]);
    for at in [60u32, 72, 0] {
        bytes.extend(at.to_be_bytes());
    }
    bytes.extend([0; 22]);
    bytes.extend(words(&[3, 0, 3, 0, -128])); // version 3, one association
    bytes.extend(words(&[0, 0, 3072, 4096, 2048, 0])); // A=2.25, B=3 pixels at 3pt
    bytes.extend(words(&[0, 0, 1, 0x4142, -1024])); // AB=-0.75 pixel at 3pt
    bytes
}

pub fn quicktime(
    codec: &[u8; 4],
    width: u16,
    height: u16,
    encoded: &[u8],
    matrix: [i32; 9],
    mask: &[u8],
) -> Vec<u8> {
    let mut payload = word(0);
    for n in matrix {
        payload.extend_from_slice(&n.to_be_bytes());
    }
    payload.extend([0; 14]); // no matte, empty matteRect, srcCopy
    payload.extend(rect(0, 0, height as i16, width as i16));
    payload.extend([0; 4]);
    payload.extend_from_slice(&(mask.len() as u32).to_be_bytes());
    payload.extend_from_slice(mask);
    let mut desc = vec![0; 86];
    desc[..4].copy_from_slice(&86u32.to_be_bytes());
    desc[4..8].copy_from_slice(codec);
    desc[32..34].copy_from_slice(&width.to_be_bytes());
    desc[34..36].copy_from_slice(&height.to_be_bytes());
    desc[44..48].copy_from_slice(&(encoded.len() as u32).to_be_bytes());
    desc[48..50].copy_from_slice(&1u16.to_be_bytes());
    desc[82..84].copy_from_slice(&24u16.to_be_bytes());
    desc[84..86].copy_from_slice(&0xffffu16.to_be_bytes());
    payload.extend(desc);
    payload.extend_from_slice(encoded);
    let mut out = (payload.len() as u32).to_be_bytes().to_vec();
    out.extend(payload);
    out
}
pub fn identity_matrix() -> [i32; 9] {
    [65536, 0, 0, 0, 65536, 0, 0, 0, 1 << 30]
}
