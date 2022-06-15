#[forbid(unsafe_code)]
pub mod map;
#[forbid(unsafe_code)]
pub mod multimap;
pub mod split;

pub use map::{CMap, CMapReader};
pub use multimap::{CMultiMap, CMultiMapReader};
