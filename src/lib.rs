// SPDX-License-Identifier: GPL-3.0-only

pub mod backend;
pub mod config;
pub mod grabs;
pub mod handlers;
pub mod input;
pub mod render;
pub mod shell;
pub mod state;

#[cfg(test)]
mod tests;

pub use state::Monotile;
