#![forbid(unsafe_code)]
//! Deterministic headless scenario runners.
//!
//! These tools compose the zone host, session runtime, protocol and control
//! plane. They add no gameplay, session or ownership rules of their own.

pub mod bots;
mod report;

pub use report::Report;
