//! A bounded PICT parser and ordered software interpreter.
//!
//! This is a documented subset, not a pixel-identical classic QuickDraw emulator.
//! Parsing is independent of rendering; unsupported drawing is never silently lost.
mod font;
mod model;
mod output;
mod parser;
mod raster;
mod vector;

pub use font::{BitmapFont, FontBook};
pub use model::{
    Container, Diagnostic, Error, ErrorCode, Fidelity, Inspection, Limits, Picture, Rect, Result,
};
pub use output::{encode_pdf, encode_png, encode_svg};
pub use parser::parse;
pub use raster::{Image, RenderOptions, Rendered, render};
pub use vector::{VectorFormat, VectorOutput, export_vector};
