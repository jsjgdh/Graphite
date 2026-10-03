use graphite_editor::test_utils::svg_comparison::ImageDiffMetrics;
use graphite_editor::test_utils::svg_comparison::{compare_svg, compare_svg_raster};

/// Channel tolerance shared by these comparisons. Large enough to absorb anti-aliasing
/// differences, tight enough to catch a shape that is actually drawn differently.
const TOLERANCE: u8 = 2;

/// Compares a sample SVG against a resvg render of Graphite's re-export of it, asserting the two
/// are pixel-equivalent within `TOLERANCE`.
async fn assert_sample_matches(name: &str, svg: &str) {
	let result = compare_svg(name, svg, TOLERANCE).await;
	assert!(result.error.is_none(), "{name} comparison error: {:?}", result.error);

	let metrics: ImageDiffMetrics = result.metrics.unwrap_or_else(|| panic!("{name} produced no metrics"));

	assert!(
		!metrics.size_mismatch(),
		"{name} rendered at different canvas sizes: resvg {}x{} vs Graphite {}x{}",
		metrics.reference_size.0,
		metrics.reference_size.1,
		metrics.graphite_size.0,
		metrics.graphite_size.1
	);
	assert!(
		metrics.passed,
		"{name} failed comparison with {:.2}% diff ({} / {} px, max channel delta {}, RMSE {:.2})",
		metrics.diff_percentage, metrics.different_pixels, metrics.total_pixels, metrics.max_channel_diff, metrics.root_mean_squared_error
	);
}

#[tokio::test]
async fn test_import_basic_shapes() {
	assert_sample_matches("shapes_basic", include_str!("../../tools/svg-import-tests/samples/shapes_basic.svg")).await;
}

#[tokio::test]
async fn test_import_path_curves() {
	assert_sample_matches("path_curves", include_str!("../../tools/svg-import-tests/samples/path_curves.svg")).await;
}

#[tokio::test]
async fn test_import_strokes() {
	assert_sample_matches("strokes", include_str!("../../tools/svg-import-tests/samples/strokes.svg")).await;
}

#[tokio::test]
async fn test_import_nested_transforms() {
	assert_sample_matches("transforms_nested", include_str!("../../tools/svg-import-tests/samples/transforms_nested.svg")).await;
}

/// Regression test for `usvg` reporting a path as invisible when it has no fill and no stroke, or
/// when its `visibility` resolves to something other than `visible`. Those paths must import as
/// hidden layers rather than being drawn.
#[tokio::test]
async fn test_import_visibility() {
	assert_sample_matches("visibility", include_str!("../../tools/svg-import-tests/samples/visibility.svg")).await;
}

/// The tests above exercise the SVG serializer. This one exercises Vello, the rasterizer the
/// editor actually draws with on screen, which is a separate render path and can disagree with the
/// serializer on the same document.
///
/// Skipped rather than failed when no GPU adapter is available, since the raster path cannot run
/// at all in that case. CI runners without a GPU will take this path.
#[tokio::test]
async fn test_import_vello_render() {
	let Some(editor) = graphite_editor::test_utils::EditorTestUtils::create_with_gpu().await else {
		eprintln!("skipping: no GPU adapter available to exercise the Vello render path");
		return;
	};
	drop(editor);

	let svg = include_str!("../../tools/svg-import-tests/samples/shapes_basic.svg");
	let result = compare_svg_raster("shapes_basic", svg, TOLERANCE).await;
	assert!(result.error.is_none(), "Vello comparison error: {:?}", result.error);

	let metrics: ImageDiffMetrics = result.metrics.expect("Vello comparison produced no metrics");
	assert!(
		!metrics.size_mismatch(),
		"Vello rendered at a different canvas size: resvg {}x{} vs Graphite {}x{}",
		metrics.reference_size.0,
		metrics.reference_size.1,
		metrics.graphite_size.0,
		metrics.graphite_size.1
	);
	// Vello and resvg anti-alias edges differently, so some edge disagreement is expected and not
	// worth failing over. What must hold is that they agree on flat interior regions, since a
	// difference there means something was drawn differently. Asserting that directly is more
	// robust than tuning a percentage threshold against two rasterizers we do not control.
	assert_eq!(
		result.interior_different_pixels, 0,
		"Vello render disagrees with resvg in {} flat interior pixel(s) (edge-adjacent diff {:.2}%, max channel delta {}): a shape was drawn with the wrong colour, size, or position",
		result.interior_different_pixels, metrics.diff_percentage, metrics.max_channel_diff
	);
}
