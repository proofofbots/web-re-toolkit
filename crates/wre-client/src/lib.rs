pub mod client;
pub mod context;
pub mod diag;
pub mod error;
pub mod proto;
pub mod shape;
pub mod sidecar;
pub mod spec;

pub use client::{Client, Registration, Registry, prepare, prepare_params};
pub use context::{
    Call, Clock, Counters, Ctx, DroppedEvents, EventSink, Fingerprint, Http, HttpOptions,
    MetricSink, Platform, Profile, Services,
};
pub use diag::{DiagConfig, DiagMode, Recorder, Report};
pub use error::{ClientError, ClientResult, ErrorKind};
pub use proto::{
    DiagReply, Envelope, Frame, HealthReply, OpenReply, OpenRequest, read_frame, write_frame,
};
pub use shape::{Field, Shape, decode_bytes, encode_bytes, field, validate};
pub use sidecar::{Session, Sidecar, SidecarOptions};
pub use spec::{
    BundleDescriptor, Capabilities, ClientDescriptor, Concurrency, EventSpec, Hello, OpSpec,
    PROTOCOL_VERSION,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
