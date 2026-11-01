//! `perch-ui` — overlay UI drawn on the glasses, kept pure so it is testable
//! without a GPU: the bottom control bar and a tiny bitmap font. The daemon's
//! render loop uploads what [`bar::Bar::render`] returns as a head-locked
//! layer and feeds pointer events to [`bar::Bar::hit`].

#![warn(missing_docs)]

pub mod bar;
pub mod font;
pub mod icons;

pub use bar::{Bar, BarAction, BarInfo, Button, Rect};
