//! Compile Typst documents into editable PowerPoint presentations.
pub mod compiler;
mod geometry;
pub mod ir;

pub use compiler::{capture, world};
