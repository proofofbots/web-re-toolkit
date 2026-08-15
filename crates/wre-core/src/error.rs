use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("json error in {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("no workspace root found from {0}, expected a wre.toml or a .git directory")]
    NoWorkspaceRoot(PathBuf),

    #[error("artifact {0} not found")]
    ArtifactMissing(String),

    #[error("capture bundle schema {found} is not supported, this build reads schema {expected}")]
    BundleSchema { found: u32, expected: u32 },

    #[error("address {0} does not resolve")]
    BadAddress(String),

    #[error("{0}")]
    Message(String),
}

impl Error {
    pub fn msg(text: impl Into<String>) -> Self {
        Error::Message(text.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Error {
    let path = path.into();
    move |source| Error::Io { path, source }
}

pub fn json(path: impl Into<PathBuf>) -> impl FnOnce(serde_json::Error) -> Error {
    let path = path.into();
    move |source| Error::Json { path, source }
}
