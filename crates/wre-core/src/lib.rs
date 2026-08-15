pub mod address;
pub mod bundle;
pub mod digest;
pub mod error;
pub mod paths;
pub mod store;

pub use address::{Address, Segment, leaves};
pub use bundle::{
    BodyRef, BrowserInfo, CaptureBundle, ConsoleRecord, CookieRecord, DocumentRecord,
    EmulationEntry, ExceptionRecord, RequestRecord, ScriptRecord, StorageRecord, ToolInfo,
};
pub use digest::{HashKind, sha256, sha256_short};
pub use error::{Error, Result};
pub use paths::{Workspace, day, safe_name, stamp};
pub use store::Store;

pub fn init_logging(default: &str) {
    use tracing_subscriber::{EnvFilter, fmt};

    let filter = EnvFilter::try_from_env("WRE_LOG").unwrap_or_else(|_| EnvFilter::new(default));

    let _ = fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init();
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
