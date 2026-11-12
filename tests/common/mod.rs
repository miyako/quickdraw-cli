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
