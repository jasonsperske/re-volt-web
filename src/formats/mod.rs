//! Parsers for Re-Volt's on-disc file formats. Layouts were checked against
//! the shipped PC v1.00 data; see docs/formats.md.

pub mod bmp;
pub mod fan;
pub mod fin;
pub mod fob;
pub mod hul;
pub mod inf;
pub mod ncp;
pub mod pan;
pub mod params;
pub mod prm;
pub mod taz;
pub mod world;

/// Little-endian cursor over a byte slice. Reads past the end are errors
/// rather than panics so a corrupt file cannot take down the page.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.remaining() < n {
            return Err(format!("unexpected end of file at byte {}", self.pos));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub fn skip(&mut self, n: usize) -> Result<(), String> {
        self.bytes(n).map(|_| ())
    }

    pub fn u8(&mut self) -> Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }

    pub fn i8(&mut self) -> Result<i8, String> {
        Ok(self.u8()? as i8)
    }

    pub fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    pub fn i16(&mut self) -> Result<i16, String> {
        Ok(self.u16()? as i16)
    }

    pub fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }

    pub fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn vec3(&mut self) -> Result<[f32; 3], String> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }
}

/// Splits a text-format line into tokens: strips `;` comments, honours single
/// and double quotes, and treats commas as whitespace (`Offset 0, 0, 0`).
pub fn tokenize(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ';' => break,
            '"' | '\'' => {
                chars.next();
                let s: String = chars.by_ref().take_while(|&q| q != c).collect();
                out.push(s);
            }
            c if c.is_whitespace() || c == ',' => {
                chars.next();
            }
            _ => {
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || c == ',' || c == ';' {
                        break;
                    }
                    s.push(c);
                    chars.next();
                }
                out.push(s);
            }
        }
    }
    out
}

pub fn text(data: &[u8]) -> String {
    // Files are 8-bit Windows text; non-ASCII only appears in comments.
    data.iter().map(|&b| b as char).collect()
}
