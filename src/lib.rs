//! A bounded PICT parser and ordered software interpreter.
//!
//! This is a documented subset, not a pixel-identical classic QuickDraw emulator.
//! Parsing is independent of rendering; unsupported drawing is never silently lost.
mod codec;
mod container;
mod fond;
mod font;
mod model;
mod outline;
mod output;
mod parser;
mod raster;
mod vector;

pub use container::{
    ContainerInventory, FontInventory, FontResourceEntry, FontResourceType, PictureEntry,
    PictureSelector, list_font_resources, list_pictures, parse_selected,
};
pub use fond::FontFamily;
pub use font::{
    BitmapFont, FontBook, FontFamilySelection, FontResourceSelection, FontScript, FontSelection,
};
pub use model::{
    Container, Diagnostic, Error, ErrorCode, Fidelity, Inspection, Limits, Picture, Rect, Result,
};
pub use outline::{OutlineEncoding, OutlineFont, OutlineSelection};
pub use output::{encode_pdf, encode_png, encode_svg};
pub use parser::parse;
pub use raster::{Image, RenderOptions, Rendered, render};
pub use vector::{VectorFormat, VectorOutput, export_vector};
