//! various strategies for sycronizing a double buffer

mod local;
#[cfg(feature = "alloc")]
mod local_tracking;
#[cfg(feature = "std")]
mod tracking;

pub use local::LocalStrategy;
#[cfg(feature = "alloc")]
pub use local_tracking::LocalTrackingStrategy;
#[cfg(feature = "std")]
pub use tracking::TrackingStrategy;
