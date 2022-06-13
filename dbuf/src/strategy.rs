//! various strategies for sycronizing a double buffer

#[cfg(feature = "alloc")]
pub mod hazard;
pub mod local;
#[cfg(feature = "alloc")]
pub mod local_hazard;
#[cfg(feature = "alloc")]
pub mod local_tracking;
#[cfg(feature = "std")]
pub mod tracking;

#[cfg(feature = "alloc")]
pub use hazard::HazardStrategy;
pub use local::LocalStrategy;
#[cfg(feature = "alloc")]
pub use local_hazard::LocalHazardStrategy;
#[cfg(feature = "alloc")]
pub use local_tracking::LocalTrackingStrategy;
#[cfg(feature = "std")]
pub use tracking::TrackingStrategy;
