use crate::messages::frontend::utility_types::{ExportBounds, FileType};
use crate::messages::portfolio::ingest::utility_types::IngestAction;
use crate::messages::prelude::*;
use crate::node_graph_executor::ExportConfig;
use crate::test_utils::EditorTestUtils;
use resvg::tiny_skia::{Color as SkiaColor, ColorU8, Pixmap, PremultipliedColorU8};
use std::collections::VecDeque;
use usvg::Transform;

#[derive(Debug, Clone)]
pub struct ImageDiffMetrics {
	/// Canvas size of the resvg render of the input SVG.
	pub reference_size: (u32, u32),
	/// Canvas size of the resvg render of Graphite's exported SVG.
	pub graphite_size: (u32, u32),
	/// Size the diff was computed over, i.e. the overlap of the two canvases.
	pub width: u32,
	pub height: u32,
	pub total_pixels: usize,
	pub different_pixels: usize,
	pub diff_percentage: f64,
	pub max_channel_diff: u8,
	pub mean_squared_error: f64,
	pub root_mean_squared_error: f64,
	pub passed: bool,
}

impl ImageDiffMetrics {
	/// The two renders disagreed on canvas size, so the pixel diff only covers the overlapping
	/// region and does not account for content that one side has and the other does not.
	pub fn size_mismatch(&self) -> bool {
		self.reference_size != self.graphite_size
	}
}

#[derive(Debug, Clone)]
pub struct SvgComparisonResult {
	pub name: String,
	pub input_svg: String,
	pub output_svg: Option<String>,
	pub metrics: Option<ImageDiffMetrics>,
	pub reference_png: Option<Vec<u8>>,
	pub graphite_png: Option<Vec<u8>>,
	pub diff_png: Option<Vec<u8>>,
	pub error: Option<String>,
}

/// Result of the raster (Vello) comparison, which exercises the renderer the editor uses on
/// screen rather than the SVG serializer.
#[derive(Debug, Clone)]
pub struct RasterComparisonResult {
	pub name: String,
	pub metrics: Option<ImageDiffMetrics>,
	/// Differing pixels that sit in a flat region of the reference, i.e. not adjacent to an edge.
	/// Vello and resvg anti-alias edges differently, so some edge disagreement is expected and not
	/// a bug; a difference in a flat interior means an actual rendering disagreement (a wrong
	/// colour, a dropped or mispositioned shape), which this is meant to catch.
	pub interior_different_pixels: usize,
	pub reference_png: Option<Vec<u8>>,
	pub graphite_png: Option<Vec<u8>>,
	pub diff_png: Option<Vec<u8>>,
	pub error: Option<String>,
}

/// Renders an SVG string directly using resvg to an RGBA Pixmap.
pub fn render_svg_with_resvg(svg_str: &str) -> Result<Pixmap, String> {
	let opt = usvg::Options::default();
	let tree = usvg::Tree::from_str(svg_str, &opt).map_err(|e| format!("usvg parse error: {e}"))?;
	let size = tree.size().to_int_size();
	let mut pixmap = Pixmap::new(size.width(), size.height()).ok_or_else(|| format!("Invalid pixmap dimensions: {}x{}", size.width(), size.height()))?;
	pixmap.fill(SkiaColor::WHITE);
	resvg::render(&tree, Transform::default(), &mut pixmap.as_mut());
	Ok(pixmap)
}

/// Compares two pixmaps, generating metrics and a visual diff pixmap.
/// `channel_tolerance` is the maximum difference allowed per channel (e.g. 2-5 to ignore minor anti-aliasing variations).
///
/// The comparison covers only the region both pixmaps have. When the two renders disagree on canvas
/// size, the metrics therefore describe the overlap alone, and `ImageDiffMetrics::size_mismatch`
/// records that content outside it went uncompared. Such a comparison is never reported as passing,
/// since a canvas size difference is itself a difference in rendered output.
pub fn compare_pixmaps(ref_pixmap: &Pixmap, actual_pixmap: &Pixmap, channel_tolerance: u8) -> Result<(ImageDiffMetrics, Pixmap), String> {
	let width = ref_pixmap.width().min(actual_pixmap.width());
	let height = ref_pixmap.height().min(actual_pixmap.height());
	if width == 0 || height == 0 {
		return Err(format!(
			"Cannot compare a {}x{} render against a {}x{} one: the two have no overlapping pixels",
			ref_pixmap.width(),
			ref_pixmap.height(),
			actual_pixmap.width(),
			actual_pixmap.height()
		));
	}
	let total_pixels = (width as usize) * (height as usize);

	let mut diff_pixmap = Pixmap::new(width, height).ok_or_else(|| format!("Failed to allocate a {width}x{height} diff image"))?;
	// Fill diff pixmap with dark background so differences stand out
	diff_pixmap.fill(SkiaColor::from_rgba8(20, 20, 25, 255));

	let mut different_pixels = 0;
	let mut max_channel_diff: u8 = 0;
	let mut sum_squared_error: f64 = 0.0;

	for y in 0..height {
		for x in 0..width {
			let (Some(ref_pixel), Some(act_pixel)) = (ref_pixmap.pixel(x, y), actual_pixmap.pixel(x, y)) else {
				continue;
			};
			let (r1, g1, b1, a1) = (ref_pixel.red(), ref_pixel.green(), ref_pixel.blue(), ref_pixel.alpha());
			let (r2, g2, b2, a2) = (act_pixel.red(), act_pixel.green(), act_pixel.blue(), act_pixel.alpha());

			let dr = r1.abs_diff(r2);
			let dg = g1.abs_diff(g2);
			let db = b1.abs_diff(b2);
			let da = a1.abs_diff(a2);

			let max_d = dr.max(dg).max(db).max(da);
			max_channel_diff = max_channel_diff.max(max_d);

			let pixel_se = ((dr as f64).powi(2) + (dg as f64).powi(2) + (db as f64).powi(2) + (da as f64).powi(2)) / 4.0;
			sum_squared_error += pixel_se;

			if max_d > channel_tolerance {
				different_pixels += 1;
				// Highlight difference in bright magenta/red
				let intensity = (max_d as f32 / 255.0).clamp(0.4, 1.0);
				let r = (255.0 * intensity) as u8;
				let b = (100.0 * intensity) as u8;
				let pixel = PremultipliedColorU8::from_rgba(r, 0, b, 255).unwrap();
				diff_pixmap.pixels_mut()[(y * width + x) as usize] = pixel;
			} else {
				// Subtle dimmed version of original
				let r = (r1 as f32 * 0.25) as u8;
				let g = (g1 as f32 * 0.25) as u8;
				let b = (b1 as f32 * 0.25) as u8;
				let a = (a1 as f32 * 0.25) as u8;
				if let Some(pixel) = PremultipliedColorU8::from_rgba(r, g, b, a.max(30)) {
					diff_pixmap.pixels_mut()[(y * width + x) as usize] = pixel;
				}
			}
		}
	}

	let mean_squared_error = if total_pixels > 0 { sum_squared_error / total_pixels as f64 } else { 0.0 };
	let root_mean_squared_error = mean_squared_error.sqrt();
	let diff_percentage = if total_pixels > 0 { (different_pixels as f64 / total_pixels as f64) * 100.0 } else { 0.0 };
	let size_mismatch = (ref_pixmap.width(), ref_pixmap.height()) != (actual_pixmap.width(), actual_pixmap.height());
	let passed = !size_mismatch && diff_percentage <= 0.1;

	Ok((
		ImageDiffMetrics {
			reference_size: (ref_pixmap.width(), ref_pixmap.height()),
			graphite_size: (actual_pixmap.width(), actual_pixmap.height()),
			width,
			height,
			total_pixels,
			different_pixels,
			diff_percentage,
			max_channel_diff,
			mean_squared_error,
			root_mean_squared_error,
			passed,
		},
		diff_pixmap,
	))
}

/// Helper to encode a tiny-skia Pixmap to PNG bytes using the image crate.
pub fn encode_pixmap_to_png(pixmap: &Pixmap) -> Result<Vec<u8>, String> {
	let mut buffer = std::io::Cursor::new(Vec::new());
	let img = image::RgbaImage::from_raw(pixmap.width(), pixmap.height(), pixmap.data().to_vec()).ok_or_else(|| "Failed to construct RgbaImage from pixmap".to_string())?;
	img.write_to(&mut buffer, image::ImageFormat::Png).map_err(|e| format!("Failed to encode PNG: {e}"))?;
	Ok(buffer.into_inner())
}

/// Runs a full comparison for an SVG string:
/// 1. Render input directly via resvg (reference)
/// 2. Import into a fresh Graphite editor instance
/// 3. Export as SVG from Graphite
/// 4. Render exported SVG via resvg (graphite output)
/// 5. Compare the two images and return full results including PNG buffers
pub async fn compare_svg(name: &str, svg_str: &str, channel_tolerance: u8) -> SvgComparisonResult {
	// Step 1: Render original SVG directly with resvg
	let ref_pixmap = match render_svg_with_resvg(svg_str) {
		Ok(p) => p,
		Err(e) => {
			return SvgComparisonResult {
				name: name.to_string(),
				input_svg: svg_str.to_string(),
				output_svg: None,
				metrics: None,
				reference_png: None,
				graphite_png: None,
				diff_png: None,
				error: Some(format!("Failed to render original SVG with resvg: {e}")),
			};
		}
	};

	// Step 2: Import into Graphite
	let mut editor = EditorTestUtils::create();
	editor
		.handle_message(IngestMessage::Ingest {
			data: svg_str.as_bytes().to_vec(),
			action: IngestAction::Open,
			mime_type: "image/svg+xml".to_string(),
			path: None,
		})
		.await;

	// Step 3: Export SVG from Graphite
	let output_svg = match export_svg_from_editor(&mut editor).await {
		Ok(svg) => svg,
		Err(e) => {
			let ref_png = encode_pixmap_to_png(&ref_pixmap).ok();
			return SvgComparisonResult {
				name: name.to_string(),
				input_svg: svg_str.to_string(),
				output_svg: None,
				metrics: None,
				reference_png: ref_png,
				graphite_png: None,
				diff_png: None,
				error: Some(format!("Failed to export SVG from Graphite: {e}")),
			};
		}
	};

	// Step 4: Render Graphite output SVG with resvg
	let graphite_pixmap = match render_svg_with_resvg(&output_svg) {
		Ok(p) => p,
		Err(e) => {
			let ref_png = encode_pixmap_to_png(&ref_pixmap).ok();
			return SvgComparisonResult {
				name: name.to_string(),
				input_svg: svg_str.to_string(),
				output_svg: Some(output_svg),
				metrics: None,
				reference_png: ref_png,
				graphite_png: None,
				diff_png: None,
				error: Some(format!("Failed to render Graphite exported SVG with resvg: {e}")),
			};
		}
	};

	// Step 5: Compare pixmaps
	let (metrics, diff_pixmap) = match compare_pixmaps(&ref_pixmap, &graphite_pixmap, channel_tolerance) {
		Ok(comparison) => comparison,
		Err(e) => {
			let ref_png = encode_pixmap_to_png(&ref_pixmap).ok();
			let graphite_png = encode_pixmap_to_png(&graphite_pixmap).ok();
			return SvgComparisonResult {
				name: name.to_string(),
				input_svg: svg_str.to_string(),
				output_svg: Some(output_svg),
				metrics: None,
				reference_png: ref_png,
				graphite_png,
				diff_png: None,
				error: Some(e),
			};
		}
	};

	let reference_png = encode_pixmap_to_png(&ref_pixmap).ok();
	let graphite_png = encode_pixmap_to_png(&graphite_pixmap).ok();
	let diff_png = encode_pixmap_to_png(&diff_pixmap).ok();

	SvgComparisonResult {
		name: name.to_string(),
		input_svg: svg_str.to_string(),
		output_svg: Some(output_svg),
		metrics: Some(metrics),
		reference_png,
		graphite_png,
		diff_png,
		error: None,
	}
}

/// Exports the active document and returns the bytes the frontend would have been handed.
async fn export_from_editor(editor: &mut EditorTestUtils, file_type: FileType) -> Result<Vec<u8>, String> {
	// Settle deferred graph runs and update metadata/bounds
	editor.eval_graph().await.map_err(|e| format!("eval_graph failed: {e}"))?;

	let portfolio = &mut editor.editor.dispatcher.message_handlers.portfolio_message_handler;
	let document_id = portfolio.active_document_id.ok_or("No active document")?;
	let (executor, documents) = (&mut portfolio.executor, &mut portfolio.documents);
	let document = documents.get_mut(&document_id).ok_or("Document not found")?;

	let export_config = ExportConfig {
		name: "export".to_string(),
		file_type,
		scale_factor: 1.,
		bounds: ExportBounds::AllArtwork,
		..Default::default()
	};

	executor
		.submit_document_export(document, document_id, export_config)
		.map_err(|e| format!("submit_document_export failed: {e}"))?;

	editor.runtime.run().await;

	let mut messages = VecDeque::new();
	editor.editor.poll_node_graph_evaluation(&mut messages).map_err(|e| format!("poll_node_graph_evaluation failed: {e}"))?;

	let mut exported = None;
	for message in messages {
		for frontend_msg in editor.editor.handle_message(message) {
			if let FrontendMessage::TriggerSaveFile { content, .. } = frontend_msg {
				exported = Some(content.to_vec());
			}
		}
	}

	exported.ok_or_else(|| format!("No TriggerSaveFile received during {file_type:?} export"))
}

/// Runs the raster (Vello) half of the comparison: render the source SVG with resvg for the
/// reference, import it into an editor with a GPU executor, export a PNG through the Vello render
/// path, and diff the two images.
pub async fn compare_svg_raster(name: &str, svg_str: &str, channel_tolerance: u8) -> RasterComparisonResult {
	let fail = |error: String, reference_png: Option<Vec<u8>>| RasterComparisonResult {
		name: name.to_string(),
		metrics: None,
		interior_different_pixels: 0,
		reference_png,
		graphite_png: None,
		diff_png: None,
		error: Some(error),
	};

	let ref_pixmap = match render_svg_with_resvg(svg_str) {
		Ok(p) => p,
		Err(e) => return fail(format!("Failed to render original SVG with resvg: {e}"), None),
	};
	let reference_png = encode_pixmap_to_png(&ref_pixmap).ok();

	let Some(mut editor) = EditorTestUtils::create_with_gpu().await else {
		return fail("No GPU adapter available, cannot exercise the Vello render path".to_string(), reference_png);
	};

	editor
		.handle_message(IngestMessage::Ingest {
			data: svg_str.as_bytes().to_vec(),
			action: IngestAction::Open,
			mime_type: "image/svg+xml".to_string(),
			path: None,
		})
		.await;

	let graphite_pixmap = match export_png_from_editor(&mut editor).await {
		Ok(p) => p,
		Err(e) => return fail(format!("Failed to export PNG from Graphite: {e}"), reference_png),
	};

	let (metrics, diff_pixmap) = match compare_pixmaps(&ref_pixmap, &graphite_pixmap, channel_tolerance) {
		Ok(comparison) => comparison,
		Err(e) => return fail(e, reference_png),
	};

	let interior_different_pixels = count_interior_differences(&ref_pixmap, &graphite_pixmap);

	RasterComparisonResult {
		name: name.to_string(),
		metrics: Some(metrics),
		interior_different_pixels,
		reference_png,
		graphite_png: encode_pixmap_to_png(&graphite_pixmap).ok(),
		diff_png: encode_pixmap_to_png(&diff_pixmap).ok(),
		error: None,
	}
}

/// Counts differing pixels that are not adjacent to an edge in the reference image.
///
/// A pixel counts as "on an edge" when any of its four neighbours in the reference differs from it
/// by more than `EDGE_THRESHOLD`. Anti-aliased boundary pixels straddle the edge and therefore
/// always have such a neighbour, so this cleanly separates rasterizer disagreement along edges
/// (expected between resvg and Vello) from a disagreement over what was actually drawn (a bug).
fn count_interior_differences(ref_pixmap: &Pixmap, actual_pixmap: &Pixmap) -> usize {
	/// Channel difference above which two neighbouring pixels are considered to be on opposite
	/// sides of an edge. Comfortably above anti-aliasing noise, well below a real colour change.
	const EDGE_THRESHOLD: u8 = 24;

	let width = ref_pixmap.width().min(actual_pixmap.width());
	let height = ref_pixmap.height().min(actual_pixmap.height());

	let channel_diff = |a: PremultipliedColorU8, b: PremultipliedColorU8| {
		a.red()
			.abs_diff(b.red())
			.max(a.green().abs_diff(b.green()))
			.max(a.blue().abs_diff(b.blue()))
			.max(a.alpha().abs_diff(b.alpha()))
	};

	let mut interior = 0;
	for y in 0..height {
		for x in 0..width {
			let (Some(ref_pixel), Some(act_pixel)) = (ref_pixmap.pixel(x, y), actual_pixmap.pixel(x, y)) else {
				continue;
			};
			if channel_diff(ref_pixel, act_pixel) == 0 {
				continue;
			}

			let on_edge = [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)].iter().any(|(dx, dy)| {
				let (nx, ny) = (x as i32 + dx, y as i32 + dy);
				if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
					return false;
				}
				ref_pixmap.pixel(nx as u32, ny as u32).is_some_and(|neighbour| channel_diff(ref_pixel, neighbour) > EDGE_THRESHOLD)
			});

			if !on_edge {
				interior += 1;
			}
		}
	}
	interior
}

pub async fn export_svg_from_editor(editor: &mut EditorTestUtils) -> Result<String, String> {
	let bytes = export_from_editor(editor, FileType::Svg).await?;
	String::from_utf8(bytes).map_err(|e| format!("Exported SVG was not valid UTF-8: {e}"))
}

/// Exports the active document as a PNG through the raster (Vello) render path.
///
/// This exercises the rasterizer the editor actually uses on screen, which is a different code
/// path from the SVG serializer: `ExportFormat::Raster` selects `RenderOutputTypeRequest::Vello`.
/// Requires an editor created via [`EditorTestUtils::create_with_gpu`]; without a GPU the runtime
/// silently falls back to the SVG path, which would make this a duplicate of
/// [`export_svg_from_editor`] and report a misleading pass.
pub async fn export_png_from_editor(editor: &mut EditorTestUtils) -> Result<Pixmap, String> {
	if !editor.has_gpu_executor {
		return Err("Editor was created without a GPU executor; use EditorTestUtils::create_with_gpu".to_string());
	}

	let bytes = export_from_editor(editor, FileType::Png).await?;
	decode_png_to_pixmap(&bytes)
}

fn decode_png_to_pixmap(bytes: &[u8]) -> Result<Pixmap, String> {
	let decoded = image::load_from_memory(bytes).map_err(|e| format!("Failed to decode exported PNG: {e}"))?;
	let rgba = decoded.to_rgba8();
	let (width, height) = rgba.dimensions();

	let mut pixmap = Pixmap::new(width, height).ok_or_else(|| format!("Invalid pixmap dimensions: {width}x{height}"))?;
	for (x, y, pixel) in rgba.enumerate_pixels() {
		// A Pixmap stores premultiplied alpha, so the decoded straight-alpha PNG bytes have to be
		// premultiplied on the way in. Skipping this makes semi-transparent pixels compare as if
		// they were far darker than they are.
		let color = ColorU8::from_rgba(pixel[0], pixel[1], pixel[2], pixel[3]).premultiply();
		pixmap.pixels_mut()[(y * width + x) as usize] = color;
	}
	Ok(pixmap)
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Builds a pixmap from an RGB function, fully opaque.
	fn pixmap_from(width: u32, height: u32, color: impl Fn(u32, u32) -> [u8; 3]) -> Pixmap {
		let mut pixmap = Pixmap::new(width, height).unwrap();
		for y in 0..height {
			for x in 0..width {
				let [r, g, b] = color(x, y);
				pixmap.pixels_mut()[(y * width + x) as usize] = ColorU8::from_rgba(r, g, b, 255).premultiply();
			}
		}
		pixmap
	}

	/// A black square on white, so the boundary is a clean vertical and horizontal edge.
	fn square(size: u32, inset: u32) -> impl Fn(u32, u32) -> [u8; 3] {
		move |x, y| {
			if x >= inset && y >= inset && x < size - inset && y < size - inset {
				[0, 0, 0]
			} else {
				[255, 255, 255]
			}
		}
	}

	#[test]
	fn identical_images_have_no_interior_differences() {
		let image = pixmap_from(32, 32, square(32, 8));
		assert_eq!(count_interior_differences(&image, &image), 0);
	}

	#[test]
	fn a_colour_change_in_the_interior_is_detected() {
		let reference = pixmap_from(32, 32, square(32, 8));
		let mut recoloured = reference.clone();
		// Change the middle of the black square, far from any edge.
		recoloured.pixels_mut()[(16 * 32 + 16) as usize] = ColorU8::from_rgba(0, 128, 0, 255).premultiply();

		assert_eq!(count_interior_differences(&reference, &recoloured), 1);
	}

	#[test]
	fn a_shift_of_a_shape_is_detected() {
		let reference = pixmap_from(32, 32, square(32, 8));
		let shifted = pixmap_from(32, 32, square(32, 10));

		// The shifted square's edges land in what was the reference's flat interior, so the
		// disagreement must be reported.
		assert!(count_interior_differences(&reference, &shifted) > 0);
	}

	#[test]
	fn differences_confined_to_an_edge_are_not_counted() {
		const SIZE: u32 = 32;
		const INSET: u32 = 8;
		let reference = pixmap_from(SIZE, SIZE, square(SIZE, INSET));
		let mut nudged = reference.clone();
		// Perturb only the left boundary column of the black square, and only along the span where
		// that column actually borders white. Extending this into the flat white above and below
		// the square would be a genuine interior difference, not an edge one.
		for y in (INSET + 1)..(SIZE - INSET - 1) {
			nudged.pixels_mut()[(y * SIZE + INSET) as usize] = ColorU8::from_rgba(40, 40, 40, 255).premultiply();
		}

		assert!(count_interior_differences(&reference, &nudged) == 0, "edge-adjacent differences must not count as interior differences");
	}
}
