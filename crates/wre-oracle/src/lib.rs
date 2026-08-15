pub mod fidelity;
pub mod signal;

pub use fidelity::{Bucket, Fidelity, Verdict, compare};
pub use signal::{Arm, Candidate, Report, Trial, find_signal};
