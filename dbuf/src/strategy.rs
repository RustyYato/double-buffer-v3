//! various strategies for sycronizing a double buffer

#[cfg(feature = "alloc")]
mod hazard;
mod local;
#[cfg(feature = "alloc")]
mod local_tracking;
#[cfg(feature = "std")]
mod tracking;

#[cfg(feature = "alloc")]
pub use hazard::HazardStrategy;
pub use local::LocalStrategy;
#[cfg(feature = "alloc")]
pub use local_tracking::LocalTrackingStrategy;
#[cfg(feature = "std")]
pub use tracking::TrackingStrategy;
