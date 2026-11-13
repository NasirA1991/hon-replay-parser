//! `.honreplay` files are ZIP archives holding `replayinfo` (XML) and `replaydata` (S2R2 binary).
use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::error::{Error, Result};

const MAX_REPLAYDATA: u64 = 1024 * 1024 * 1024;

pub struct Archive {
    pub replayinfo: String,
    pub replaydata: Vec<u8>,
    pub compressed_bytes: u64,
}

impl Archive {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::open(path)?;
        Self::from_reader(file)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Self::from_reader(std::io::Cursor::new(bytes))
    }

    fn from_reader<R: std::io::Read + std::io::Seek>(reader: R) -> Result<Self> {
        let mut zip = zip::ZipArchive::new(reader)?;
        let mut replayinfo = String::new();
        if let Ok(mut entry) = zip.by_name("replayinfo") {
            let mut raw = Vec::new();
            entry.read_to_end(&mut raw)?;
            replayinfo = String::from_utf8_lossy(&raw).into_owned();
        }
        let mut entry = zip
            .by_name("replaydata")
            .map_err(|_| Error::MissingEntry("replaydata"))?;
        if entry.size() > MAX_REPLAYDATA {
            return Err(Error::malformed(0, "replaydata is unreasonably large"));
        }
        let compressed_bytes = entry.compressed_size();
        let mut replaydata = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut replaydata)?;
        Ok(Self {
            replayinfo,
            replaydata,
            compressed_bytes,
        })
    }
}
