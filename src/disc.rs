//! Reads the ISO9660 filesystem from the game's data track.
//!
//! Accepts either a raw CD image track (2352-byte sectors, as in the shipped
//! `.bin`/`.cue` set) or a cooked `.iso` (2048-byte sectors). Byte access goes
//! through [`ByteSource`] so the browser can serve reads lazily from a `File`
//! without copying the whole 160 MB track into WASM memory.

use std::collections::BTreeMap;

#[allow(async_fn_in_trait)] // only used within this crate, single-threaded
pub trait ByteSource {
    async fn read(&self, offset: u64, len: usize) -> Result<Vec<u8>, String>;
}

const USER_DATA: usize = 2048;
const SYNC: [u8; 12] = [0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectorFormat {
    /// Bytes per sector in the image.
    pub stride: usize,
    /// Offset of the 2048 bytes of user data inside each sector.
    pub data_offset: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct FileEntry {
    pub lba: u32,
    pub size: u32,
}

pub struct Disc<S> {
    source: S,
    format: SectorFormat,
    /// Upper-case, `/`-separated paths from the disc root.
    files: BTreeMap<String, FileEntry>,
}

impl<S: ByteSource> Disc<S> {
    pub async fn open(source: S) -> Result<Self, String> {
        let head = source.read(0, 16).await?;
        let format = if head.len() == 16 && head[..12] == SYNC {
            match head[15] {
                1 => SectorFormat { stride: 2352, data_offset: 16 },
                2 => SectorFormat { stride: 2352, data_offset: 24 },
                m => return Err(format!("unsupported CD sector mode {m}")),
            }
        } else {
            SectorFormat { stride: USER_DATA, data_offset: 0 }
        };
        let mut disc = Disc { source, format, files: BTreeMap::new() };

        let pvd = disc.read_sectors(16, 1).await?;
        if &pvd[1..6] != b"CD001" || pvd[0] != 1 {
            return Err("not an ISO9660 data track".into());
        }
        let root = pvd[156..156 + 34].to_vec();
        let (lba, size) = extent(&root);
        // Breadth-first walk; directories are small so this is a handful of reads.
        let mut pending = vec![(String::new(), lba, size)];
        while let Some((prefix, lba, size)) = pending.pop() {
            let data = disc.read_sectors(lba, size.div_ceil(USER_DATA as u32)).await?;
            let mut i = 0usize;
            while i < size as usize {
                let len = data[i] as usize;
                if len == 0 {
                    // Records never straddle sectors; skip the padding.
                    i = (i / USER_DATA + 1) * USER_DATA;
                    continue;
                }
                let rec = &data[i..i + len];
                i += len;
                let name_len = rec[32] as usize;
                let raw_name = &rec[33..33 + name_len];
                if raw_name == [0] || raw_name == [1] {
                    continue;
                }
                let name = String::from_utf8_lossy(raw_name);
                let name = name.split(';').next().unwrap_or("").to_ascii_uppercase();
                let path = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
                let (clba, csize) = extent(rec);
                if rec[25] & 2 != 0 {
                    pending.push((path, clba, csize));
                } else {
                    disc.files.insert(path, FileEntry { lba: clba, size: csize });
                }
            }
        }
        Ok(disc)
    }

    pub fn format(&self) -> SectorFormat {
        self.format
    }

    pub fn files(&self) -> impl Iterator<Item = (&str, &FileEntry)> {
        self.files.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn exists(&self, path: &str) -> bool {
        self.files.contains_key(&normalize(path))
    }

    /// Immediate children (files and directories) of a directory path.
    pub fn list_dir(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{}/", normalize(dir));
        let mut out: Vec<String> = self
            .files
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix))
            .map(|rest| rest.split('/').next().unwrap().to_string())
            .collect();
        out.dedup();
        out
    }

    pub async fn read_file(&self, path: &str) -> Result<Vec<u8>, String> {
        let key = normalize(path);
        let entry = *self.files.get(&key).ok_or_else(|| format!("file not on disc: {key}"))?;
        let mut data = self.read_sectors(entry.lba, entry.size.div_ceil(USER_DATA as u32)).await?;
        data.truncate(entry.size as usize);
        Ok(data)
    }

    async fn read_sectors(&self, lba: u32, count: u32) -> Result<Vec<u8>, String> {
        let SectorFormat { stride, data_offset } = self.format;
        let raw = self.source.read(lba as u64 * stride as u64, count as usize * stride).await?;
        if raw.len() < count as usize * stride {
            return Err(format!("short read at sector {lba}"));
        }
        if stride == USER_DATA {
            return Ok(raw);
        }
        let mut out = Vec::with_capacity(count as usize * USER_DATA);
        for sector in raw.chunks_exact(stride) {
            out.extend_from_slice(&sector[data_offset..data_offset + USER_DATA]);
        }
        Ok(out)
    }
}

/// Maps game-style paths (`cars\Adeon\body.prm`) and disc paths to the index key.
pub fn normalize(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches('/').to_ascii_uppercase()
}

fn extent(rec: &[u8]) -> (u32, u32) {
    let lba = u32::from_le_bytes(rec[2..6].try_into().unwrap());
    let size = u32::from_le_bytes(rec[10..14].try_into().unwrap());
    (lba, size)
}
