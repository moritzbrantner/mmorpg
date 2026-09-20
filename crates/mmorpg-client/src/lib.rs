#![forbid(unsafe_code)]

pub mod graphics;
pub mod network;
pub mod presentation;

pub type ClientError = Box<dyn std::error::Error + Send + Sync>;
