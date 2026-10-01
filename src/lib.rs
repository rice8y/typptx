//! Compile Typst documents into editable PowerPoint presentations.
mod assets;
pub mod compiler;
mod geometry;
pub mod graphics;
pub mod ir;
pub mod lower;
pub mod math;
pub mod pptx;

pub use compiler::{capture, world};
