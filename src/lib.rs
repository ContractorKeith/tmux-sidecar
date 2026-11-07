#![cfg_attr(not(test), warn(clippy::unwrap_used))]

pub mod cli;
pub mod client;
pub mod daemon;
pub mod doctor;
pub mod engine;
pub mod hook;
pub mod install;
pub mod mcp;
pub mod model;
pub mod notify;
pub mod paths;
pub mod proc;
pub mod rpc;
pub mod seams;
pub mod store;
pub mod tmux;
pub mod ui;
