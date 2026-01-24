use crate::blending_nodes::blend_colors;
use crate::filter::gaussian_blur_algorithm;
use core_types::blending::BlendMode;
use core_types::color::Color;
use core_types::context::Ctx;
use core_types::table::Table;
use glam::DVec2;
use no_std_types::shadow::ShadowType;
use raster_types::Raster;
use raster_types::{Bitmap, BitmapMut, CPU, Image};

#[node_macro::node(category("Raster: Adjustment"))]
fn drop_shadow(
	_: impl Ctx,
	image: Table<Raster<CPU>>,
	color: Color,
	blur_radius: f64,
	spread: f64,
	offset: DVec2,
	light_position: DVec2,
	use_light_source: bool,
	shadow_type: ShadowType,
	opacity: f32,
) -> Table<Raster<CPU>> {
	image
		.into_iter()
		.map(|mut row| {
			let original_image = &row.element;
			let width = original_image.width;
			let height = original_image.height;

			if width == 0 || height == 0 {
				return row;
			}

			// 1. Extract Mask (Alpha Channel)
			let mut shadow_image = Image::new(width, height, Color::TRANSPARENT);
			for y in 0..height {
				for x in 0..width {
					if let Some(pixel) = original_image.get_pixel(x, y) {
						let alpha = pixel.a();
						if alpha > 0. {
							shadow_image.set_pixel(x, y, color.with_alpha(alpha));
						}
					}
				}
			}

			// 2. Spread (Dilation)
			let spread_image = if spread > 0. { dilate_algorithm(shadow_image, spread) } else { shadow_image };

			// 3. Blur
			let blurred_shadow = if blur_radius > 0. {
				gaussian_blur_algorithm(spread_image, blur_radius, false)
			} else {
				spread_image
			};

			// 4. Calculate Offset
			let shadow_offset = if use_light_source {
				// Torch Logic: Shadow is cast opposite to the light position relative to object center (0,0)
				// Offset = -LightPosition
				-light_position
			} else {
				offset
			};

			// 5. Composite
			let mut final_image = Image::new(width, height, Color::TRANSPARENT);

			for y in 0..height {
				for x in 0..width {
					let main_pixel = original_image.get_pixel(x, y).unwrap_or(Color::TRANSPARENT);

					let shadow_x = x as f64 - shadow_offset.x;
					let shadow_y = y as f64 - shadow_offset.y;

					let s_x = shadow_x.round() as i32;
					let s_y = shadow_y.round() as i32;

					let shadow_pixel = if s_x >= 0 && s_x < width as i32 && s_y >= 0 && s_y < height as i32 {
						blurred_shadow.get_pixel(s_x as u32, s_y as u32).unwrap_or(Color::TRANSPARENT)
					} else {
						Color::TRANSPARENT
					};

					let shadow_pixel = shadow_pixel.with_alpha(shadow_pixel.a() * opacity);

					let result_pixel = match shadow_type {
						ShadowType::Drop => blend_colors(main_pixel, shadow_pixel, BlendMode::Normal, 1.0),
						ShadowType::Inner => {
							let mixed = blend_colors(shadow_pixel, main_pixel, BlendMode::Normal, 1.0);
							mixed.with_alpha(mixed.a() * main_pixel.a())
						}
					};

					final_image.set_pixel(x, y, result_pixel);
				}
			}

			row.element = Raster::new_cpu(final_image);
			row
		})
		.collect()
}

fn dilate_algorithm(original_buffer: Image<Color>, radius: f64) -> Image<Color> {
	let (width, height) = original_buffer.dimensions();
	let mut output = Image::new(width, height, Color::TRANSPARENT);
	let radius_ceil = radius.ceil() as i32;
	let radius_sq = radius * radius;

	for y in 0..height {
		for x in 0..width {
			// Optimization: Check center first. If fully opaque, no need to search neighbors if we just max.
			// However for correct distance based dilation we should search.
			// Simple box/circle dilation: max alpha in neighborhood.

			let mut max_alpha = 0.0;
			let mut max_color = Color::TRANSPARENT;

			// Optimization range check
			let min_dy = (-radius_ceil).max(-(y as i32));
			let max_dy = radius_ceil.min((height as i32) - 1 - (y as i32));
			let min_dx = (-radius_ceil).max(-(x as i32));
			let max_dx = radius_ceil.min((width as i32) - 1 - (x as i32));

			'search: for dy in min_dy..=max_dy {
				for dx in min_dx..=max_dx {
					if (dx * dx) as f64 + (dy * dy) as f64 > radius_sq {
						continue;
					}

					let ny = y as i32 + dy;
					let nx = x as i32 + dx;

					// Unsafe get could be used here since we clamped loops, but keeping safe for now.
					// We already clamped ranges so ny/nx are valid.
					if let Some(pixel) = original_buffer.get_pixel(nx as u32, ny as u32) {
						if pixel.a() > max_alpha {
							max_alpha = pixel.a();
							max_color = pixel;
							if max_alpha >= 1.0 {
								break 'search;
							}
						}
					}
				}
			}

			if max_alpha > 0. {
				// We keep the color we found (which is the shadow color with some alpha)
				output.set_pixel(x, y, max_color);
			}
		}
	}
	output
}
