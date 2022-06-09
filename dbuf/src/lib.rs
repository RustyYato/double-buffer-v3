//! an implementation of double buffers which is usable even in no_std contexts

#![no_std]
#![forbid(
    clippy::undocumented_unsafe_blocks,
    unsafe_op_in_unsafe_fn,
    clippy::missing_safety_doc,
    clippy::missing_docs_in_private_items
)]

pub mod raw;
pub mod strategy;

pub mod interface;
