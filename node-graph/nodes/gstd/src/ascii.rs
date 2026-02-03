use core_types::color::Color;
use core_types::context::Ctx;
use core_types::table::Table;
use glam::DVec2;
use graphic_types::Vector;
use graphic_types::raster_types::{Bitmap, CPU, Raster};
use graphic_types::vector_types::subpath::{ManipulatorGroup, Subpath};
use graphic_types::vector_types::vector::PointId;

/// Converts an image to ASCII art, rendered as vector shapes.
#[node_macro::node(category("Raster: Effect"))]
fn ascii_art(
	_: impl Ctx,
	/// The input raster image to convert to ASCII art.
	image_frame: Table<Raster<CPU>>,
	/// Number of pixels per ASCII character cell.
	#[default(8.)]
	#[hard_min(1.)]
	#[hard_max(32.)]
	cell_size: f64,
	/// Contrast adjustment factor.
	#[default(1.2)]
	#[hard_min(0.5)]
	#[hard_max(3.0)]
	contrast: f64,
) -> Table<Vector> {
	const ASCII_CHARS: &[u8] = b" .,:;i1tfLCG08@";

	let cell_size_u32 = cell_size as u32;

	// Get first image from table
	let Some(first_row) = image_frame.into_iter().next() else {
		return Table::default();
	};

	let image = first_row.element.into_data();
	let (img_width, img_height) = image.dimensions();

	if img_width == 0 || img_height == 0 {
		return Table::default();
	}

	let ascii_cols: u32 = (img_width / cell_size_u32).max(1);
	let ascii_rows: u32 = (img_height / cell_size_u32).max(1);

	let get_luminance = |pixel: &Color| -> f64 { 0.299 * pixel.r() as f64 + 0.587 * pixel.g() as f64 + 0.114 * pixel.b() as f64 };
	let apply_contrast = |lum: f64| -> f64 { ((lum - 0.5) * contrast + 0.5).clamp(0.0, 1.0) };

	// Sample a cell and return average luminance
	let sample_cell = |cell_x: u32, cell_y: u32| -> f64 {
		let start_x = cell_x * cell_size_u32;
		let start_y = cell_y * cell_size_u32;
		let end_x = (start_x + cell_size_u32).min(img_width);
		let end_y = (start_y + cell_size_u32).min(img_height);

		let mut total_lum = 0.0f64;
		let mut count = 0u32;

		for y in start_y..end_y {
			for x in start_x..end_x {
				if let Some(pixel) = image.get_pixel(x, y) {
					total_lum += get_luminance(&pixel);
					count += 1;
				}
			}
		}

		if count == 0 {
			return 0.0;
		}
		apply_contrast(total_lum / count as f64)
	};

	let mut subpaths: Vec<Subpath<PointId>> = Vec::new();
	let mut point_id_gen = PointId::ZERO;

	// Helper to add a rectangle subpath
	let mut add_rect = |x: f64, y: f64, w: f64, h: f64, paths: &mut Vec<Subpath<PointId>>| {
		let p0 = DVec2::new(x, y);
		let p1 = DVec2::new(x + w, y);
		let p2 = DVec2::new(x + w, y + h);
		let p3 = DVec2::new(x, y + h);

		let subpath = Subpath::new(
			vec![
				ManipulatorGroup::new_anchor_with_id(p0, point_id_gen.next_id()),
				ManipulatorGroup::new_anchor_with_id(p1, point_id_gen.next_id()),
				ManipulatorGroup::new_anchor_with_id(p2, point_id_gen.next_id()),
				ManipulatorGroup::new_anchor_with_id(p3, point_id_gen.next_id()),
			],
			true,
		);
		paths.push(subpath);
	};

	// Helper to add a character at position
	let mut add_char = |c: u8, x: f64, y: f64, size: f64, paths: &mut Vec<Subpath<PointId>>| {
		// Simple "stick" font definitions based on 3x5 grid mostly
		let s = size / 5.0; // scale unit

		match c {
			b' ' => {}
			b'.' => add_rect(x + 2. * s, y + 4. * s, s, s, paths),
			b',' => add_rect(x + 2. * s, y + 4. * s, s, 2. * s, paths),
			b':' => {
				add_rect(x + 2. * s, y + 1. * s, s, s, paths);
				add_rect(x + 2. * s, y + 4. * s, s, s, paths);
			}
			b';' => {
				add_rect(x + 2. * s, y + 1. * s, s, s, paths);
				add_rect(x + 2. * s, y + 4. * s, s, 2. * s, paths);
			}
			b'i' => {
				add_rect(x + 2. * s, y + 0.5 * s, s, 0.5 * s, paths); // dot
				add_rect(x + 2. * s, y + 2. * s, s, 3. * s, paths); // body
			}
			b'1' => add_rect(x + 2. * s, y, s, 5. * s, paths),
			b't' => {
				add_rect(x + 2. * s, y, s, 5. * s, paths); // vertical
				add_rect(x + 1. * s, y + 1.5 * s, 3. * s, s, paths); // cross
			}
			b'f' => {
				add_rect(x + 2. * s, y, s, 5. * s, paths); // vertical
				add_rect(x + 2. * s, y, 2. * s, s, paths); // top
				add_rect(x + 1. * s, y + 2. * s, 3. * s, s, paths); // mid
			}
			b'L' => {
				add_rect(x + 1. * s, y, s, 5. * s, paths);
				add_rect(x + 1. * s, y + 4. * s, 3. * s, s, paths);
			}
			b'C' => {
				add_rect(x + 1. * s, y, s, 5. * s, paths); // left
				add_rect(x + 1. * s, y, 3. * s, s, paths); // top
				add_rect(x + 1. * s, y + 4. * s, 3. * s, s, paths); // bottom
			}
			b'G' => {
				add_rect(x + 1. * s, y, s, 5. * s, paths); // left
				add_rect(x + 1. * s, y, 3. * s, s, paths); // top
				add_rect(x + 1. * s, y + 4. * s, 3. * s, s, paths); // bottom
				add_rect(x + 3. * s, y + 3. * s, s, 2. * s, paths); // hook
			}
			b'0' => {
				add_rect(x + 1. * s, y, s, 5. * s, paths); // left
				add_rect(x + 3. * s, y, s, 5. * s, paths); // right
				add_rect(x + 1. * s, y, 3. * s, s, paths); // top
				add_rect(x + 1. * s, y + 4. * s, 3. * s, s, paths); // bottom
			}
			b'8' => {
				add_rect(x + 1. * s, y, s, 5. * s, paths); // left
				add_rect(x + 3. * s, y, s, 5. * s, paths); // right
				add_rect(x + 1. * s, y, 3. * s, s, paths); // top
				add_rect(x + 1. * s, y + 2. * s, 3. * s, s, paths); // mid
				add_rect(x + 1. * s, y + 4. * s, 3. * s, s, paths); // bottom
			}
			b'@' => {
				add_rect(x + 0.5 * s, y + 0.5 * s, 4. * s, 4. * s, paths); // block
				add_rect(x + 1.5 * s, y + 1.5 * s, 2. * s, 2. * s, paths); // inner hole simulation (actually logic obscures this in filling, but we are just adding paths. For simple filling, overlapping paths might cancel out or fill. Let's just do a block for @ for maximum density)
			}
			_ => add_rect(x + 1. * s, y + 2. * s, 3. * s, s, paths), // dash default
		}
	};

	let font_size = cell_size * 0.8; // slightly smaller than cell to leave gap

	for row in 0..ascii_rows {
		for col in 0..ascii_cols {
			let lum = sample_cell(col, row);
			let char_idx = ((lum * (ASCII_CHARS.len() - 1) as f64).round() as usize).min(ASCII_CHARS.len() - 1);
			let char = ASCII_CHARS[char_idx];

			if char != b' ' {
				add_char(char, col as f64 * cell_size, row as f64 * cell_size, font_size, &mut subpaths);
			}
		}
	}

	if subpaths.is_empty() {
		return Table::default();
	}

	let vector = Vector::from_subpaths(subpaths, false);
	Table::new_from_element(vector)
}
