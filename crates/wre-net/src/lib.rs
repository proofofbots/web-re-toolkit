pub mod emulate;
pub mod h2;
pub mod hpack;
pub mod http;
pub mod proxy;
pub mod tls;

pub use emulate::{Fingerprint, Platform, Profile};
pub use h2::{Frame, FrameKind, H2Fingerprint, fingerprint_bytes};
pub use http::{CHROME_UA, Client, ClientOptions, FetchRequest, FetchResponse};
pub use proxy::{ProxyScheme, ProxySpec, random_session};
pub use tls::{ClientHello, ClientHelloBuilder, ClientHelloSummary, Ja3, is_grease};
