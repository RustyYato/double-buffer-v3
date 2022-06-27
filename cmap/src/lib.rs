#[forbid(unsafe_code)]
pub mod map;
#[forbid(unsafe_code)]
pub mod multimap;
pub mod split;

pub type DefaultHasher = std::collections::hash_map::RandomState;
pub type DefaultStrat = dbuf::strategy::HazardStrategy<dbuf::wait::DefaultWait>;

pub use map::{CMap, CMapReader};
pub use multimap::{CMultiMap, CMultiMapReader};
