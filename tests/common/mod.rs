//! Shared helpers: serve a local disc image to the engine. Point REVOLT_DISC
//! at the data track (Track 01 .bin or an .iso); tests skip if it's absent.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Mutex;

use revolt_web::assets::GameData;
use revolt_web::disc::ByteSource;

pub struct FileSource(Mutex<File>);

impl ByteSource for FileSource {
    async fn read(&self, offset: u64, len: usize) -> Result<Vec<u8>, String> {
        let mut f = self.0.lock().unwrap();
        f.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        let mut buf = Vec::with_capacity(len);
        f.by_ref().take(len as u64).read_to_end(&mut buf).map_err(|e| e.to_string())?;
        Ok(buf)
    }
}

pub fn open_game() -> Option<GameData<FileSource>> {
    let path = std::env::var("REVOLT_DISC")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/Re-Volt (USA) (En,Fr,De,Es,It) (Track 01).bin").into());
    if !std::path::Path::new(&path).exists() {
        eprintln!("no disc image at {path}; skipping");
        return None;
    }
    let file = File::open(path).unwrap();
    Some(pollster::block_on(GameData::open(FileSource(Mutex::new(file)))).unwrap())
}
