use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("archive is missing the `{0}` entry")]
    MissingEntry(&'static str),
    #[error("replaydata does not start with S2R2 (found {0:?})")]
    BadMagic([u8; 4]),
    #[error("unsupported S2R2 format version {0}")]
    UnsupportedVersion(u32),
    #[error("invalid replayinfo xml: {0}")]
    InvalidInfo(String),
    #[error("malformed replay at {offset}: {message}")]
    Malformed { offset: usize, message: String },
    #[error("entity stream desynchronised in frame {frame} at bit {bit}: {message}")]
    Desync {
        frame: u64,
        bit: usize,
        message: String,
    },
}

impl Error {
    pub fn malformed(offset: usize, message: impl Into<String>) -> Self {
        Error::Malformed {
            offset,
            message: message.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
