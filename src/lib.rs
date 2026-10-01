//! Compile Typst documents into editable PowerPoint presentations.
mod assets;
pub mod compiler;
mod geometry;
pub mod ir;
pub mod math;

pub use compiler::{capture, world};
