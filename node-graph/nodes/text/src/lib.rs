mod font_cache;
mod path_builder;
mod text_context;
mod to_path;

use dyn_any::DynAny;
pub use font_cache::*;
pub use text_context::TextContext;
pub use to_path::*;

// Re-export for convenience
pub use core_types as gcore;
pub use vector_types;

// Import specta so derive macros can find it
use core_types::specta;

/// Alignment of lines of type within a text block.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, Hash, DynAny, core_types::specta::Type, node_macro::ChoiceType)]
#[widget(Radio)]
pub enum TextAlign {
	#[default]
	Left,
	Center,
	Right,
	#[label("Justify")]
	JustifyLeft,
	// TODO: JustifyCenter, JustifyRight, JustifyAll
}

impl From<TextAlign> for parley::Alignment {
	fn from(val: TextAlign) -> Self {
		match val {
			TextAlign::Left => parley::Alignment::Left,
			TextAlign::Center => parley::Alignment::Center,
			TextAlign::Right => parley::Alignment::Right,
			TextAlign::JustifyLeft => parley::Alignment::Justify,
		}
	}
}

#[derive(PartialEq, Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct TypesettingConfig {
	pub font_size: f64,
	pub line_height_ratio: f64,
	pub character_spacing: f64,
	pub max_width: Option<f64>,
	pub max_height: Option<f64>,
	pub tilt: f64,
	pub align: TextAlign,
}

impl Default for TypesettingConfig {
	fn default() -> Self {
		Self {
			font_size: 24.,
			line_height_ratio: 1.2,
			character_spacing: 0.,
			max_width: None,
			max_height: None,
			tilt: 0.,
			align: TextAlign::default(),
		}
	}
}

/// Per-character or per-range style attributes for rich text.
/// All fields are optional to allow sparse styling (only override specific attributes).
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize, DynAny, specta::Type)]
pub struct TextStyle {
	/// Font override for this range.
	pub font: Option<Font>,
	/// Font size in pixels.
	pub size: Option<f64>,
	/// Text color.
	pub color: Option<core_types::Color>,
	/// Line height ratio (relative to font size).
	pub line_height: Option<f64>,
	/// Additional letter spacing in pixels.
	pub letter_spacing: Option<f64>,
}

/// A styled span defining a range of text and its styling.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, DynAny, specta::Type)]
pub struct StyleSpan {
	/// Start byte index (inclusive).
	pub start: usize,
	/// End byte index (exclusive).
	pub end: usize,
	/// Style to apply to this range.
	pub style: TextStyle,
}

/// Styled text containing the raw string and a list of style spans.
/// Spans may overlap; later spans take precedence for conflicting attributes.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize, DynAny, specta::Type)]
pub struct StyledText {
	/// The raw text content.
	pub text: String,
	/// List of style spans to apply.
	pub spans: Vec<StyleSpan>,
}

impl From<String> for StyledText {
	fn from(text: String) -> Self {
		Self { text, spans: Vec::new() }
	}
}

impl From<&str> for StyledText {
	fn from(text: &str) -> Self {
		Self {
			text: text.to_string(),
			spans: Vec::new(),
		}
	}
}
