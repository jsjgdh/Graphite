use crate::renderer::{
	ClearGuardPlacement, ItemRef, RenderParams, composite_paint_colors, faded_paint_color, format_transform_matrix, gradient_placement, gradient_settings_from_item, spread_adjusted_samples,
	transform_is_invertible,
};
use crate::{Render, RenderSvgSegmentList, SvgRender};
use core_types::color::SRGBA8;
use core_types::list::List;
use core_types::uuid::generate_uuid;
use core_types::{ATTR_GRADIENT_FORM, ATTR_GRADIENT_UNITS, ATTR_TRANSFORM, Color};
use glam::{DAffine2, DVec2};
use graphic_types::Graphic;
use graphic_types::vector_types::gradient::{GradientForm, GradientUnits};
use graphic_types::vector_types::vector::style::{Stroke, StrokeAlign, StrokeCap, StrokeJoin};
use std::fmt::Write;
use vector_types::Gradient;
use vector_types::gradient::GradientSpread;

#[derive(Copy, Clone, PartialEq)]
pub enum PaintTarget {
	Fill,
	Stroke,
}

impl PaintTarget {
	fn paint_attr(self) -> &'static str {
		match self {
			Self::Fill => "fill",
			Self::Stroke => "stroke",
		}
	}

	fn opacity_attr(self) -> &'static str {
		match self {
			Self::Fill => "fill-opacity",
			Self::Stroke => "stroke-opacity",
		}
	}
}

pub trait RenderExt {
	type Output;

	#[allow(clippy::too_many_arguments)]
	fn render(
		&self,
		svg_defs: &mut String,
		item_transform: DAffine2,
		element_transform: DAffine2,
		stroke_transform: DAffine2,
		bounds: DAffine2,
		render_params: &RenderParams,
		target: PaintTarget,
	) -> Self::Output;
}

/// The paint attribute for an already-faded solid color, or the SVG `none` keyword when the color is absent.
fn render_color_paint(color: Option<Color>, target: PaintTarget) -> String {
	let Some(color) = color else { return format!(r#" {}="none""#, target.paint_attr()) };

	let mut result = format!(r##" {}="#{}""##, target.paint_attr(), SRGBA8::from(color).to_rgb_hex());
	if color.a() < 1. {
		let _ = write!(result, r#" {}="{}""#, target.opacity_attr(), (color.a() * 1000.).round() / 1000.);
	}

	result
}

impl RenderExt for List<Color> {
	type Output = String;

	fn render(
		&self,
		_svg_defs: &mut String,
		_item_transform: DAffine2,
		_element_transform: DAffine2,
		_stroke_transform: DAffine2,
		_bounds: DAffine2,
		render_params: &RenderParams,
		target: PaintTarget,
	) -> Self::Output {
		render_color_paint(composite_paint_colors(self, render_params.for_mask), target)
	}
}

/// Adds one gradient item's def into `svg_defs` and returns the gradient ID, or `None` when the item is absent.
/// `for_mask` keeps the fill opacity at full, as [`ItemRef::paint_opacity`] explains.
fn render_gradient_paint(item: Option<ItemRef<'_, Gradient>>, svg_defs: &mut String, item_transform: DAffine2, element_transform: DAffine2, bounds: DAffine2, for_mask: bool) -> Option<u64> {
	let mut stop = String::new();

	let item = item?;
	let stops = item.element()?;
	let gradient_form: GradientForm = item.attribute_cloned_or_default(ATTR_GRADIENT_FORM);
	let gradient_units: GradientUnits = item.attribute_cloned_or_default(ATTR_GRADIENT_UNITS);
	let local_gradient_transform: DAffine2 = item.attribute_cloned_or_default(ATTR_TRANSFORM);
	let settings = gradient_settings_from_item(item);

	let (mut samples, _) = spread_adjusted_samples(stops, settings, gradient_form, ClearGuardPlacement::SvgStopOrder);

	let paint_opacity = item.paint_opacity(for_mask);
	if paint_opacity < 1. {
		for (_, color, _) in &mut samples {
			*color = color.with_alpha(color.a() * paint_opacity);
		}
	}

	for (position, color, original_midpoint) in samples {
		stop.push_str("<stop");
		if position != 0. {
			let _ = write!(stop, r#" offset="{}""#, (position * 1_000_000.).round() / 1_000_000.);
		}
		let _ = write!(stop, r##" stop-color="#{}""##, SRGBA8::from(color).to_rgb_hex());
		if color.a() < 1. {
			let _ = write!(stop, r#" stop-opacity="{}""#, (color.a() * 1000.).round() / 1000.);
		}
		if let Some(midpoint) = original_midpoint {
			let _ = write!(stop, r#" graphite:midpoint="{}""#, (midpoint * 1000.).round() / 1000.);
		}
		stop.push_str(" />")
	}

	// A gradient with no stops paints as solid black, matching `Gradient::evaluate` (a stopless def would otherwise render as no paint per the SVG spec)
	if stop.is_empty() {
		stop.push_str(r##"<stop stop-color="#000000""##);
		if paint_opacity < 1. {
			let _ = write!(stop, r#" stop-opacity="{}""#, (paint_opacity * 1000.).round() / 1000.);
		}
		stop.push_str(" />");
	}

	// Need to cancel out the element's transform as it is already applied to the path itself.
	let element_transform_inverse = if transform_is_invertible(element_transform) {
		element_transform.inverse()
	} else {
		DAffine2::IDENTITY
	};

	let document_transform = item_transform * local_gradient_transform;

	let placement = gradient_placement(document_transform, gradient_form);
	let placement = element_transform_inverse * placement;

	// The unit gradient is written as x1/y1/x2/y2 over the 0..1 range, which SVG interprets in whichever coordinate system
	// `gradientUnits` names. To export as `objectBoundingBox`, fold the shape's own bounding box out of the placement so the
	// result is expressed as fractions of that box, matching how the source spelled it.
	let (gradient_units, gradient_transform) = match gradient_units {
		GradientUnits::UserSpaceOnUse => (GradientUnits::UserSpaceOnUse, format_transform_matrix(placement)),
		GradientUnits::ObjectBoundingBox if transform_is_invertible(bounds) => (GradientUnits::ObjectBoundingBox, format_transform_matrix(bounds.inverse() * placement)),
		// A degenerate box can't be divided out of, so fall back to the equivalent user-space spelling.
		GradientUnits::ObjectBoundingBox => (GradientUnits::UserSpaceOnUse, format_transform_matrix(placement)),
	};
	let gradient_units = format!(r#" gradientUnits="{}""#, gradient_units.svg_name());
	let gradient_transform = if gradient_transform.is_empty() {
		String::new()
	} else {
		format!(r#" gradientTransform="{gradient_transform}""#)
	};

	let gradient_spread = if matches!(settings.spread, GradientSpread::Pad | GradientSpread::Clear) {
		String::new()
	} else {
		format!(r#" spreadMethod="{}""#, settings.spread.svg_name())
	};

	let gradient_id = generate_uuid();

	match gradient_form {
		GradientForm::Linear => {
			let _ = write!(
				svg_defs,
				r#"<linearGradient id="{}"{} x1="0" y1="0" x2="1" y2="0"{gradient_spread}{gradient_transform}>{}</linearGradient>"#,
				gradient_id, gradient_units, stop
			);
		}
		GradientForm::Radial => {
			let _ = write!(
				svg_defs,
				r#"<radialGradient id="{}"{} cx="0" cy="0" r="1"{gradient_spread}{gradient_transform}>{}</radialGradient>"#,
				gradient_id, gradient_units, stop
			);
		}
	}

	Some(gradient_id)
}

impl RenderExt for List<Gradient> {
	type Output = Option<u64>;

	/// Adds the gradient def through mutating the first argument, returning the gradient ID, or `None` when the list is empty.
	fn render(
		&self,
		svg_defs: &mut String,
		item_transform: DAffine2,
		element_transform: DAffine2,
		_stroke_transform: DAffine2,
		bounds: DAffine2,
		render_params: &RenderParams,
		_target: PaintTarget,
	) -> Self::Output {
		render_gradient_paint(
			(!self.is_empty()).then_some(ItemRef::ListItem(self, 0)),
			svg_defs,
			item_transform,
			element_transform,
			bounds,
			render_params.for_mask,
		)
	}
}

impl RenderExt for Stroke {
	type Output = String;

	/// Provide the shape-related SVG attributes for the stroke. The paint-related attributes for the stroke are generated from `Graphic::render` with `PaintTarget::Stroke`.
	fn render(
		&self,
		_svg_defs: &mut String,
		_item_transform: DAffine2,
		_element_transform: DAffine2,
		_stroke_transform: DAffine2,
		_bounds: DAffine2,
		render_params: &RenderParams,
		_target: PaintTarget,
	) -> Self::Output {
		// Don't render a stroke at all if it would be invisible
		if !self.has_renderable_stroke() {
			return String::new();
		}

		let default_weight = if self.align != StrokeAlign::Center && render_params.aligned_strokes { 1. / 2. } else { 1. };

		// Set to None if the value is the SVG default
		let weight = (self.weight != default_weight).then_some(self.weight);
		let dash_array = (!self.dash_lengths.is_empty()).then_some(self.dash_lengths());
		let dash_offset = (self.dash_offset != 0.).then_some(self.dash_offset);
		let stroke_cap = (self.cap != StrokeCap::Butt).then_some(self.cap);
		let stroke_join = (self.join != StrokeJoin::Miter).then_some(self.join);
		let stroke_join_miter_limit = (self.join_miter_limit != 4.).then_some(self.join_miter_limit);
		let stroke_align = (self.align != StrokeAlign::Center).then_some(self.align);

		// Render the needed stroke attributes
		let mut attributes = String::new();
		if let Some(mut weight) = weight {
			if stroke_align.is_some() && render_params.aligned_strokes {
				weight *= 2.;
			}
			let _ = write!(&mut attributes, r#" stroke-width="{weight}""#);
		}
		if let Some(dash_array) = dash_array {
			let _ = write!(&mut attributes, r#" stroke-dasharray="{dash_array}""#);
		}
		if let Some(dash_offset) = dash_offset {
			let _ = write!(&mut attributes, r#" stroke-dashoffset="{dash_offset}""#);
		}
		if let Some(stroke_cap) = stroke_cap {
			let _ = write!(&mut attributes, r#" stroke-linecap="{}""#, stroke_cap.svg_name());
		}
		if let Some(stroke_join) = stroke_join {
			let _ = write!(&mut attributes, r#" stroke-linejoin="{}""#, stroke_join.svg_name());
		}
		if let Some(stroke_join_miter_limit) = stroke_join_miter_limit {
			let _ = write!(&mut attributes, r#" stroke-miterlimit="{stroke_join_miter_limit}""#);
		}
		if render_params.stroke_below {
			let _ = write!(&mut attributes, r#" style="paint-order: stroke;" "#);
		}
		attributes
	}
}

impl RenderExt for Graphic {
	type Output = String;

	fn render(
		&self,
		svg_defs: &mut String,
		item_transform: DAffine2,
		element_transform: DAffine2,
		stroke_transform: DAffine2,
		bounds: DAffine2,
		render_params: &RenderParams,
		target: PaintTarget,
	) -> Self::Output {
		let paint_attr = target.paint_attr();

		match self {
			Graphic::Color(item) => render_color_paint(faded_paint_color(ItemRef::Item(item), render_params.for_mask), target),
			Graphic::ColorList(color_list) => color_list.render(svg_defs, item_transform, element_transform, stroke_transform, bounds, render_params, target),
			Graphic::Gradient(item) => render_gradient_paint(Some(ItemRef::Item(item)), svg_defs, item_transform, element_transform, bounds, render_params.for_mask)
				.map(|gradient_id| format!(r##" {paint_attr}="url(#{gradient_id})""##))
				.unwrap_or_else(|| format!(r#" {paint_attr}="none""#)),
			// One gradient resolves to a paint server; stacking several needs them composited, which only the pattern below can do
			Graphic::GradientList(gradient_list) if gradient_list.len() <= 1 => gradient_list
				.render(svg_defs, item_transform, element_transform, stroke_transform, bounds, render_params, target)
				.map(|gradient_id| format!(r##" {paint_attr}="url(#{gradient_id})""##))
				.unwrap_or_else(|| format!(r#" {paint_attr}="none""#)),
			Graphic::None(_) | Graphic::NoneList(_) => format!(r#" {paint_attr}="none""#),
			Graphic::Graphic(_)
			| Graphic::Vector(_)
			| Graphic::RasterCPU(_)
			| Graphic::RasterGPU(_)
			| Graphic::Text(_)
			| Graphic::VectorList(_)
			| Graphic::RasterCPUList(_)
			| Graphic::RasterGPUList(_)
			| Graphic::GraphicList(_)
			| Graphic::GradientList(_)
			| Graphic::TextList(_)
			| Graphic::StrokeList(_) => {
				let bounds = if target == PaintTarget::Stroke {
					// To prevent a wraparound artefact occurring when the tile boundary and the stroke region are perfectly aligned, the local coordinate is expanded slightly.
					let inverse = |len: f64| if len > 0. { 1. / len } else { 0. };
					let inflate = DVec2::new(inverse(item_transform.matrix2.x_axis.length()), inverse(item_transform.matrix2.y_axis.length()));
					let min = bounds.transform_point2(DVec2::ZERO) - inflate;
					let max = bounds.transform_point2(DVec2::ONE) + inflate;
					DAffine2::from_scale_angle_translation(max - min, 0., min)
				} else {
					bounds
				};
				render_svg_pattern(svg_defs, self, stroke_transform, bounds, render_params)
					.map(|id| format!(r##" {paint_attr}="url(#{id})""##))
					.unwrap_or_else(|| format!(r#" {paint_attr}="none""#))
			}
		}
	}
}

/// Emits an SVG `<pattern>` paint server into `svg_defs` that renders the given graphic as the paint content, and returns the pattern ID.
/// Currently, this function is only used for clipping-based filling and stroking, not considering tiling yet.
fn render_svg_pattern(svg_defs: &mut String, paint: &Graphic, stroke_transform: DAffine2, bounds: DAffine2, render_params: &RenderParams) -> Option<String> {
	let min = bounds.transform_point2(DVec2::ZERO);
	let max = bounds.transform_point2(DVec2::ONE);
	let size = max - min;
	if size.x <= 0. || size.y <= 0. {
		return None;
	}

	// Render the pattern content recursively
	let mut content = SvgRender::new();
	paint.render_svg(&mut content, &render_params.for_pattern());

	// Unwrap the inner def element
	write!(svg_defs, "{}", content.svg_defs).unwrap();

	let pattern_transform = stroke_transform * DAffine2::from_translation(min);
	let transform_str = format_transform_matrix(pattern_transform);
	let transform_attr = if transform_str.is_empty() {
		String::new()
	} else {
		format!(r#" patternTransform="{transform_str}""#)
	};

	let pattern_id = format!("pattern-{}", generate_uuid());
	write!(
		svg_defs,
		r##"<pattern id="{pattern_id}" patternUnits="userSpaceOnUse" x="0" y="0" width="{}" height="{}"{transform_attr}>"##,
		size.x, size.y,
	)
	.unwrap();

	let content_shift = format_transform_matrix(DAffine2::from_translation(-min));
	write!(svg_defs, r##"<g transform="{content_shift}">{}</g></pattern>"##, content.svg.to_svg_string()).unwrap();

	Some(pattern_id)
}

#[cfg(test)]
mod tests {
	use super::*;
	use core_types::list::Item;

	/// Builds one gradient item whose own placement is `transform`, rendered into `svg_defs` and returned.
	fn render_def(units: GradientUnits, transform: DAffine2, bounds: DAffine2) -> String {
		let item = Item::new_from_element(Gradient::from(vec![Color::BLACK, Color::WHITE]))
			.with_attribute(ATTR_GRADIENT_FORM, GradientForm::Linear)
			.with_attribute(ATTR_GRADIENT_UNITS, units)
			.with_attribute(ATTR_TRANSFORM, transform);

		let mut svg_defs = String::new();
		render_gradient_paint(Some(ItemRef::Item(&item)), &mut svg_defs, DAffine2::IDENTITY, DAffine2::IDENTITY, bounds, false).expect("the item is present");

		svg_defs
	}

	#[test]
	fn user_space_units_keep_the_placement_as_is() {
		let defs = render_def(
			GradientUnits::UserSpaceOnUse,
			DAffine2::from_scale_angle_translation(DVec2::splat(2.), 0., DVec2::new(3., 5.)),
			DAffine2::IDENTITY,
		);

		assert!(defs.contains(r#" gradientUnits="userSpaceOnUse""#), "the default spelling should be written, got {defs}");
		assert!(
			defs.contains(&format!(
				r#"gradientTransform="{}""#,
				format_transform_matrix(DAffine2::from_scale_angle_translation(DVec2::splat(2.), 0., DVec2::new(3., 5.)))
			)),
			"the placement should be written in user units, got {defs}"
		);
	}

	#[test]
	fn bounding_box_units_fold_the_shapes_box_out_of_the_placement() {
		let placement = DAffine2::from_scale_angle_translation(DVec2::splat(2.), 0., DVec2::new(3., 5.));
		let bounds = DAffine2::from_scale_angle_translation(DVec2::new(10., 20.), 0., DVec2::new(100., 200.));

		let defs = render_def(GradientUnits::ObjectBoundingBox, placement, bounds);

		assert!(defs.contains(r#" gradientUnits="objectBoundingBox""#), "the source's spelling should be preserved, got {defs}");
		// Dividing the placement by the box expresses the same gradient as fractions of that box
		assert!(
			defs.contains(&format!(r#"gradientTransform="{}""#, format_transform_matrix(bounds.inverse() * placement))),
			"the placement should be expressed in bounding box fractions, got {defs}"
		);
	}

	#[test]
	fn a_degenerate_box_falls_back_to_the_equivalent_user_space_spelling() {
		let placement = DAffine2::from_scale_angle_translation(DVec2::splat(2.), 0., DVec2::new(3., 5.));

		let defs = render_def(GradientUnits::ObjectBoundingBox, placement, DAffine2::ZERO);

		assert!(defs.contains(r#" gradientUnits="userSpaceOnUse""#), "an unsizable box can't take the fractional spelling, got {defs}");
		assert!(
			defs.contains(&format!(r#"gradientTransform="{}""#, format_transform_matrix(placement))),
			"the placement should still be written in user units, got {defs}"
		);
	}
}
