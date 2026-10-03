mod report;

use clap::Parser;
use graphite_editor::test_utils::svg_comparison::compare_svg;
use report::{SvgTestRunResult, TestStatus, generate_csv_report, generate_html_report, summarize_by_category};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(name = "svg-import-tests", about = "Graphite SVG Import Test Suite & Visual Comparison Tool")]
struct Args {
	/// Directory containing SVG test files
	#[arg(short, long)]
	dir: Option<PathBuf>,

	/// Single SVG file to test
	#[arg(short, long)]
	file: Option<PathBuf>,

	/// Output directory for HTML report and diff images
	#[arg(short, long, default_value = "target/svg-test-report")]
	output: PathBuf,

	/// Maximum channel color delta to treat as identical (0-255)
	#[arg(short, long, default_value_t = 2)]
	tolerance: u8,

	/// Maximum allowed difference percentage (0-100) before marking a test as failed
	#[arg(short, long, default_value_t = 0.5)]
	max_diff: f64,

	/// Automatically skip SVGs containing <text> elements or in text subdirectories
	#[arg(long, default_value_t = false)]
	skip_text: bool,

	/// Automatically skip SVGs containing <filter> elements or in filters subdirectories
	#[arg(long, default_value_t = false)]
	skip_filters: bool,

	/// Recursively search for SVGs in subdirectories
	#[arg(short, long, default_value_t = false)]
	recursive: bool,

	/// Skip the HTML visual comparison report (it is written by default)
	#[arg(long, action = clap::ArgAction::SetTrue)]
	no_html: bool,

	/// Save individual PNG image files to the output directory
	#[arg(long, default_value_t = false)]
	write_images: bool,

	/// Do not exit with non-zero code on test failures
	#[arg(long, default_value_t = false)]
	allow_failures: bool,
}

#[tokio::main]
async fn main() {
	env_logger::init();
	let args = Args::parse();

	println!("============================================================");
	println!("        Graphite SVG Import & Render Test Suite             ");
	println!("============================================================");

	let mut svg_files = Vec::new();
	// The directory the scan started from, used to derive each test's category.
	let scan_root: Option<PathBuf>;

	if let Some(file_path) = &args.file {
		if file_path.is_file() {
			svg_files.push(file_path.clone());
			scan_root = file_path.parent().map(Path::to_path_buf);
		} else {
			eprintln!("Error: Specified file does not exist: {}", file_path.display());
			std::process::exit(1);
		}
	} else if let Some(dir_path) = &args.dir {
		if dir_path.is_dir() {
			collect_svg_files(dir_path, args.recursive, &mut svg_files);
			scan_root = Some(dir_path.clone());
		} else {
			eprintln!("Error: Specified directory does not exist: {}", dir_path.display());
			std::process::exit(1);
		}
	} else {
		// Default to the built-in samples directory
		let samples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("samples");
		if samples_dir.exists() {
			println!("No directory specified. Using sample test SVGs from: {}", samples_dir.display());
			collect_svg_files(&samples_dir, true, &mut svg_files);
			scan_root = Some(samples_dir);
		} else {
			eprintln!("Error: No input specified and samples directory not found at {}", samples_dir.display());
			std::process::exit(1);
		}
	}
	let scan_root = scan_root.as_deref();

	if svg_files.is_empty() {
		println!("No SVG files found to test.");
		return;
	}

	println!("Found {} SVG file(s) to test.", svg_files.len());
	println!("Channel tolerance: {} | Max allowed diff: {:.2}%\n", args.tolerance, args.max_diff);

	fs::create_dir_all(&args.output).unwrap_or_else(|e| {
		eprintln!("Failed to create output directory {}: {e}", args.output.display());
	});

	let mut results = Vec::new();
	let total_start = Instant::now();

	for (i, path) in svg_files.iter().enumerate() {
		let test_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
		print!("[{:>3}/{:>3}] {:<35} ... ", i + 1, svg_files.len(), test_name);

		let svg_content = match fs::read_to_string(path) {
			Ok(c) => c,
			Err(e) => {
				println!("\x1b[31m[ERROR: Failed to read file: {e}]\x1b[0m");
				results.push(SvgTestRunResult {
					name: test_name,
					path: path.display().to_string(),
					category: category(path, scan_root),
					status: TestStatus::Error(format!("Read error: {e}")),
					metrics: None,
					reference_png: None,
					graphite_png: None,
					diff_png: None,
					input_svg: String::new(),
					output_svg: None,
					error_message: Some(e.to_string()),
				});
				continue;
			}
		};

		let is_text = path.to_string_lossy().contains("/text/") || svg_content.contains("<text") || svg_content.contains("<tspan");
		let is_filter = path.to_string_lossy().contains("/filters/") || svg_content.contains("<filter") || svg_content.contains("<fe");

		// Skips are recorded as results rather than dropped, so the progress denominator, the
		// summary, and the HTML report all agree on how many files were considered.
		let skip_reason = if args.skip_text && is_text {
			Some("contains text")
		} else if args.skip_filters && is_filter {
			Some("contains filters")
		} else {
			None
		};
		if let Some(reason) = skip_reason {
			println!("\x1b[33m[SKIP]\x1b[0m {reason}");
			results.push(SvgTestRunResult {
				name: test_name,
				path: path.display().to_string(),
				category: category(path, scan_root),
				status: TestStatus::Skipped(reason.to_string()),
				metrics: None,
				reference_png: None,
				graphite_png: None,
				diff_png: None,
				input_svg: String::new(),
				output_svg: None,
				error_message: None,
			});
			continue;
		}

		let comparison = compare_svg(&test_name, &svg_content, args.tolerance).await;

		if let Some(err) = comparison.error {
			println!("\x1b[31m[ERROR: {}]\x1b[0m", err);
			results.push(SvgTestRunResult {
				name: test_name,
				path: path.display().to_string(),
				category: category(path, scan_root),
				status: TestStatus::Error(err.clone()),
				metrics: None,
				reference_png: comparison.reference_png,
				graphite_png: comparison.graphite_png,
				diff_png: comparison.diff_png,
				input_svg: svg_content,
				output_svg: comparison.output_svg,
				error_message: Some(err),
			});
		} else if let Some(metrics) = comparison.metrics {
			// A canvas size mismatch is a failure in its own right, independent of the pixel
			// percentage: the two renders disagree about the size of the output, and the pixel diff
			// only covers their overlap.
			let size_mismatch = metrics.size_mismatch();
			let passed = !size_mismatch && metrics.diff_percentage <= args.max_diff;
			let status = if passed { TestStatus::Passed } else { TestStatus::Failed };

			let size_note = if size_mismatch {
				format!(
					", SIZE MISMATCH: resvg {}x{} vs Graphite {}x{}",
					metrics.reference_size.0, metrics.reference_size.1, metrics.graphite_size.0, metrics.graphite_size.1
				)
			} else {
				String::new()
			};

			if passed {
				println!(
					"\x1b[32m[PASS]\x1b[0m diff: {:.2}% ({} px, delta: {})",
					metrics.diff_percentage, metrics.different_pixels, metrics.max_channel_diff
				);
			} else {
				println!(
					"\x1b[31m[FAIL]\x1b[0m diff: {:.2}% ({} / {} px, delta: {}, RMSE: {:.2}){size_note}",
					metrics.diff_percentage, metrics.different_pixels, metrics.total_pixels, metrics.max_channel_diff, metrics.root_mean_squared_error
				);
			}

			if args.write_images {
				if let Some(ref_png) = &comparison.reference_png {
					let _ = fs::write(args.output.join(format!("{test_name}_ref.png")), ref_png);
				}
				if let Some(g_png) = &comparison.graphite_png {
					let _ = fs::write(args.output.join(format!("{test_name}_graphite.png")), g_png);
				}
				if let Some(diff_png) = &comparison.diff_png {
					let _ = fs::write(args.output.join(format!("{test_name}_diff.png")), diff_png);
				}
			}

			results.push(SvgTestRunResult {
				name: test_name,
				path: path.display().to_string(),
				category: category(path, scan_root),
				status,
				metrics: Some(metrics),
				reference_png: comparison.reference_png,
				graphite_png: comparison.graphite_png,
				diff_png: comparison.diff_png,
				input_svg: svg_content,
				output_svg: comparison.output_svg,
				error_message: None,
			});
		}
	}

	let duration = total_start.elapsed();
	let total = results.len();
	let passed = results.iter().filter(|r| r.status == TestStatus::Passed).count();
	let failed = results.iter().filter(|r| r.status == TestStatus::Failed).count();
	let skipped = results.iter().filter(|r| matches!(r.status, TestStatus::Skipped(_))).count();
	let errors = results.iter().filter(|r| matches!(r.status, TestStatus::Error(_))).count();

	println!("\n============================================================");
	println!("                      TEST SUMMARY                          ");
	println!("============================================================");
	println!("Total:   {}", total);
	println!("Passed:  \x1b[32m{}\x1b[0m", passed);
	println!("Failed:  \x1b[31m{}\x1b[0m", failed);
	println!("Skipped: \x1b[33m{}\x1b[0m", skipped);
	println!("Errors:  \x1b[35m{}\x1b[0m", errors);
	println!("Time:    {:.2?}", duration);

	// The per-category rollup is the actionable part: it shows which areas of SVG support have the
	// most room to improve, ordered worst-first.
	let summaries = summarize_by_category(&results);
	if summaries.len() > 1 {
		println!();
		println!("By category (worst first):");
		println!("{:<24} {:>6} {:>6} {:>6} {:>6} {:>6} {:>10}", "Category", "Total", "Pass", "Fail", "Skip", "Error", "Pass Rate");
		for s in &summaries {
			let rate = if s.total == 0 { 0.0 } else { s.passed as f64 / s.total as f64 * 100.0 };
			println!("{:<24} {:>6} {:>6} {:>6} {:>6} {:>6} {:>9.2}%", s.category, s.total, s.passed, s.failed, s.skipped, s.errors, rate);
		}
	}

	let csv_path = args.output.join("report.csv");
	if let Err(e) = fs::write(&csv_path, generate_csv_report(&summaries)) {
		eprintln!("Failed to write CSV report: {e}");
	} else {
		println!("\nCSV summary: {}", csv_path.display());
	}

	if !args.no_html {
		let html_report = generate_html_report(&results, "All Tests", false);
		let all_report_path = args.output.join("all-report.html");
		if let Err(e) = fs::write(&all_report_path, &html_report) {
			eprintln!("Failed to write HTML report: {e}");
		} else {
			println!("\nAll tests HTML report: {}", all_report_path.display());
		}

		let failed_results: Vec<SvgTestRunResult> = results.iter().filter(|r| r.status == TestStatus::Failed || matches!(r.status, TestStatus::Error(_))).cloned().collect();
		let failed_report = generate_html_report(&failed_results, "Failed Tests Only", true);
		let failed_report_path = args.output.join("failed-report.html");
		if let Err(e) = fs::write(&failed_report_path, failed_report) {
			eprintln!("Failed to write failed tests HTML report: {e}");
		} else {
			println!("Failed tests HTML report: {}", failed_report_path.display());
		}
	}

	if !args.allow_failures && (failed > 0 || errors > 0) {
		std::process::exit(1);
	}
}

/// Groups a test by its first path component beneath the scanned root. That is the level the
/// external corpora group by (`tests/painting`, `tests/shapes`), and it is far more useful than
/// grouping by the immediate parent, which fragments `tests/filters` into one row per `fe*`
/// subdirectory and buries the categories that actually need work.
///
/// Files sitting directly in the root are grouped as `root`.
fn category(path: &Path, root: Option<&Path>) -> String {
	let relative = root.and_then(|root| path.strip_prefix(root).ok()).unwrap_or(path);
	relative
		.components()
		.next()
		.and_then(|component| match component {
			std::path::Component::Normal(name) => name.to_str(),
			_ => None,
		})
		.filter(|name| !name.is_empty())
		.unwrap_or("root")
		.to_string()
}

fn collect_svg_files(dir: &Path, recursive: bool, list: &mut Vec<PathBuf>) {
	if let Ok(entries) = fs::read_dir(dir) {
		let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
		files.sort();

		for path in files {
			if path.is_file() && path.extension().and_then(|s| s.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("svg")) {
				list.push(path);
			} else if recursive && path.is_dir() {
				collect_svg_files(&path, true, list);
			}
		}
	}
}
