#![forbid(unsafe_code)]

pub mod camera;
pub mod graphics;
pub mod network;
pub mod presentation;
pub mod session;
pub mod unit_models;
pub mod world;

pub type ClientError = Box<dyn std::error::Error + Send + Sync>;
