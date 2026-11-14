//! Ошибки: каждая говорит, что случилось и что сделать.

use std::io;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot access `{path}`: {source}")]
    Io {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("`{0}` is not a project file (the signature at the start of the file is wrong); was it saved as text or renamed from another format?")]
    NotAProjectFile(String),
    #[error("the file was written in format version {found}, this program reads up to version {supported}; update the engine")]
    UnsupportedVersion { found: u16, supported: u16 },
    #[error("the file is cut off {at}; the copy or download was probably interrupted — get the file again")]
    Truncated { at: String },
    #[error("entry `{entry}` is damaged: {what}")]
    Corrupt { entry: String, what: String },
    #[error("the file list of the project file is damaged: {0}")]
    Manifest(String),
    #[error("unsafe path `{path}` in the file: {why}; the file was not unpacked")]
    UnsafePath { path: String, why: &'static str },
    #[error("`{a}` and `{b}` differ only in letter case and cannot live in one folder on Windows or macOS")]
    CaseCollision { a: String, b: String },
    #[error("the project has {found} files, more than the limit of {limit}; exclude folders (tests, builds) or raise the limit")]
    TooManyFiles { found: u64, limit: u64 },
    #[error("the project is {found} bytes unpacked, more than the limit of {limit}; a bigger archive is refused as a possible decompression bomb")]
    TooLarge { found: u64, limit: u64 },
    #[error("entry `{entry}` is longer than the file list says ({declared} bytes); refusing to unpack it")]
    LongerThanDeclared { entry: String, declared: u64 },
    #[error("`{0}` has no Cargo.toml; choose the project folder")]
    NoProject(String),
    #[error("`{path}` would be written through a link to another folder; refusing to unpack there")]
    LinkInTheWay { path: String },
    #[error("`{0}` already exists; choose another name or let the existing files be skipped or overwritten")]
    Exists(String),
    #[error("cancelled")]
    Cancelled,
}

impl Error {
    pub(crate) fn io(path: impl AsRef<std::path::Path>, source: io::Error) -> Self {
        Error::Io {
            path: path.as_ref().display().to_string(),
            source,
        }
    }
}
