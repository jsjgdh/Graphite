use base64::prelude::*;
use graphite_editor::test_utils::svg_comparison::ImageDiffMetrics;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TestStatus {
	Passed,
	Failed,
	Skipped(String),
	Error(String),
}

#[derive(Debug, Clone)]
pub struct SvgTestRunResult {
	pub name: String,
	pub path: String,
	/// Top-level directory the file lives in, used to group results (e.g. `painting`, `shapes`).
	/// Files with no meaningful parent directory are grouped under `ungrouped`.
	pub category: String,
	pub status: TestStatus,
	pub metrics: Option<ImageDiffMetrics>,
	pub reference_png: Option<Vec<u8>>,
	pub graphite_png: Option<Vec<u8>>,
	pub diff_png: Option<Vec<u8>>,
	pub input_svg: String,
	pub output_svg: Option<String>,
	pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CategorySummary {
	pub category: String,
	pub total: usize,
	pub passed: usize,
	pub failed: usize,
	pub skipped: usize,
	pub errors: usize,
}

impl CategorySummary {
	fn pass_rate(&self) -> f64 {
		if self.total == 0 { 0.0 } else { self.passed as f64 / self.total as f64 * 100.0 }
	}

	fn pass_rate_label(&self) -> String {
		format!("{:.2}%", self.pass_rate())
	}
}

/// Rolls results up by category, so a contributor can see which area of SVG support needs work
/// without reading every individual test. Categories are ordered by pass rate ascending, so the
/// areas with the most room to improve come first.
pub fn summarize_by_category(results: &[SvgTestRunResult]) -> Vec<CategorySummary> {
	let mut summaries: Vec<CategorySummary> = Vec::new();
	for result in results {
		let summary = match summaries.iter_mut().find(|s| s.category == result.category) {
			Some(existing) => existing,
			None => {
				summaries.push(CategorySummary {
					category: result.category.clone(),
					total: 0,
					passed: 0,
					failed: 0,
					skipped: 0,
					errors: 0,
				});
				summaries.last_mut().expect("just pushed a summary")
			}
		};
		summary.total += 1;
		match result.status {
			TestStatus::Passed => summary.passed += 1,
			TestStatus::Failed => summary.failed += 1,
			TestStatus::Skipped(_) => summary.skipped += 1,
			TestStatus::Error(_) => summary.errors += 1,
		}
	}
	summaries.sort_by(|a, b| a.pass_rate().partial_cmp(&b.pass_rate()).unwrap_or(std::cmp::Ordering::Equal));
	summaries
}

/// Renders the category rollup as CSV, suitable for attaching to an issue or pasting in chat.
pub fn generate_csv_report(summaries: &[CategorySummary]) -> String {
	let mut csv = String::from("Category,Total,Passed,Failed,Skipped,Errors,Pass Rate\n");
	for s in summaries {
		csv.push_str(&format!(
			"{},{},{},{},{},{},{:.2}%\n",
			csv_field(&s.category),
			s.total,
			s.passed,
			s.failed,
			s.skipped,
			s.errors,
			s.pass_rate()
		));
	}
	csv
}

fn csv_field(field: &str) -> String {
	// Quote when the value could otherwise break the row, and double any embedded quotes.
	if field.contains([',', '"', '\n', '\r']) {
		format!("\"{}\"", field.replace('"', "\"\""))
	} else {
		field.to_string()
	}
}

pub fn generate_html_report(results: &[SvgTestRunResult], title: &str, show_back_button: bool) -> String {
	let total = results.len();
	let passed = results.iter().filter(|r| r.status == TestStatus::Passed).count();
	let failed = results.iter().filter(|r| r.status == TestStatus::Failed).count();
	let skipped = results.iter().filter(|r| matches!(r.status, TestStatus::Skipped(_))).count();
	let errors = results.iter().filter(|r| matches!(r.status, TestStatus::Error(_))).count();
	let summaries = summarize_by_category(results);

	let nav_html = if show_back_button {
		r##"<div style="display: flex; gap: 0.75rem; align-items: center; margin-top: 0.5rem;">
			<a href="all-report.html" class="nav-btn">All tests</a>
		</div>"##
			.to_string()
	} else {
		String::new()
	};

	// The per-category rollup is the part worth reading: it points at which areas of SVG support
	// need work without requiring a click into every individual test.
	let summary_table = if summaries.len() > 1 {
		let rows = summaries
			.iter()
			.map(|s| {
				format!(
					r##"<tr>
					<td><a href="#category-{category}">{category}</a></td>
					<td>{total}</td>
					<td class="num pass">{passed}</td>
					<td class="num fail">{failed}</td>
					<td class="num skip">{skipped}</td>
					<td class="num err">{errors}</td>
					<td class="num rate">{rate}</td>
				</tr>"##,
					category = html_escape(&s.category),
					total = s.total,
					passed = s.passed,
					failed = s.failed,
					skipped = s.skipped,
					errors = s.errors,
					rate = s.pass_rate_label(),
				)
			})
			.collect::<String>();
		format!(
			r#"<table class="summary-table">
				<thead><tr><th>Category</th><th>Total</th><th>Pass</th><th>Fail</th><th>Skip</th><th>Error</th><th>Pass Rate</th></tr></thead>
				<tbody>{rows}</tbody>
			</table>"#
		)
	} else {
		String::new()
	};

	let mut test_cards = String::new();
	for r in results {
		let (status_class, status_badge) = match &r.status {
			TestStatus::Passed => ("passed", "PASSED"),
			TestStatus::Failed => ("failed", "FAILED"),
			TestStatus::Skipped(reason) => ("skipped", reason.as_str()),
			TestStatus::Error(reason) => ("error", reason.as_str()),
		};

		// The badge already carries the failure reason, so the body only adds detail when there is
		// no badge text to read (a skipped or errored run with a message attached).
		let metrics_html = if let Some(m) = &r.metrics {
			let size_note = if m.size_mismatch() {
				format!(
					r#"<span class="size-mismatch">Size mismatch: resvg {}x{} vs Graphite {}x{} (diff covers the {}x{} overlap only)</span>"#,
					m.reference_size.0, m.reference_size.1, m.graphite_size.0, m.graphite_size.1, m.width, m.height
				)
			} else {
				String::new()
			};
			format!(
				r#"<div class="metrics">
					<span>Diff: <strong>{:.2}%</strong> ({} / {} px)</span>
					<span>Max Delta: <strong>{}</strong></span>
					<span>RMSE: <strong>{:.2}</strong></span>
					<span>Size: <strong>{}x{}</strong></span>
					{size_note}
				</div>"#,
				m.diff_percentage, m.different_pixels, m.total_pixels, m.max_channel_diff, m.root_mean_squared_error, m.width, m.height
			)
		} else if let Some(err) = &r.error_message {
			format!(r#"<div class="error-msg">{}</div>"#, html_escape(err))
		} else {
			String::new()
		};

		let ref_src = r.reference_png.as_ref().map(|b| format!("data:image/png;base64,{}", BASE64_STANDARD.encode(b))).unwrap_or_default();
		let graph_src = r.graphite_png.as_ref().map(|b| format!("data:image/png;base64,{}", BASE64_STANDARD.encode(b))).unwrap_or_default();
		let diff_src = r.diff_png.as_ref().map(|b| format!("data:image/png;base64,{}", BASE64_STANDARD.encode(b))).unwrap_or_default();

		let preview_html = if r.reference_png.is_some() || r.graphite_png.is_some() {
			format!(
				r#"<div class="previews">
					<div class="preview-col">
						<h4>Reference (resvg)</h4>
						<div class="img-wrapper">
							{}
						</div>
					</div>
					<div class="preview-col">
						<h4>Graphite Render</h4>
						<div class="img-wrapper">
							{}
						</div>
					</div>
					<div class="preview-col">
						<h4>Visual Diff</h4>
						<div class="img-wrapper diff-bg">
							{}
						</div>
					</div>
				</div>"#,
				if !ref_src.is_empty() {
					format!(r#"<img src="{ref_src}" alt="Reference" />"#)
				} else {
					"<em>N/A</em>".to_string()
				},
				if !graph_src.is_empty() {
					format!(r#"<img src="{graph_src}" alt="Graphite" />"#)
				} else {
					"<em>N/A</em>".to_string()
				},
				if !diff_src.is_empty() {
					format!(r#"<img src="{diff_src}" alt="Diff" />"#)
				} else {
					"<em>N/A</em>".to_string()
				},
			)
		} else {
			String::new()
		};

		let sources_html = format!(
			r#"<details class="code-details">
				<summary>View SVG Code</summary>
				<div class="code-split">
					<div>
						<h5>Input SVG</h5>
						<pre><code>{}</code></pre>
					</div>
					<div>
						<h5>Graphite Output SVG</h5>
						<pre><code>{}</code></pre>
					</div>
				</div>
			</details>"#,
			html_escape(&r.input_svg),
			r.output_svg.as_deref().map(html_escape).unwrap_or_else(|| "N/A".to_string())
		);

		test_cards.push_str(&format!(
			r#"<div class="test-card status-{status_class}" id="category-{category}" data-status="{status_class}" data-name="{name}">
				<div class="card-header">
					<span class="test-name">{name} <small class="test-path">({path})</small></span>
					<span class="badge badge-{status_class}">{status_badge}</span>
				</div>
				<div class="card-body">
					{metrics_html}
					{preview_html}
					{sources_html}
				</div>
			</div>"#,
			category = html_escape(&r.category),
			name = html_escape(&r.name),
			path = html_escape(&r.path),
		));
	}

	format!(
		r##"<!DOCTYPE html>
<html lang="en">
<head>
	<meta charset="UTF-8">
	<meta name="viewport" content="width=device-width, initial-scale=1.0">
	<title>{title} - Graphite SVG Import Test Report</title>
	<style>
		:root {{
			--bg-main: #121316;
			--bg-card: #1c1d22;
			--bg-card-header: #262830;
			--text-main: #f0f1f5;
			--text-muted: #9ba1b0;
			--border-color: #313540;
			--accent-blue: #3d82f6;
			--pass-color: #22c55e;
			--fail-color: #ef4444;
			--skip-color: #eab308;
			--error-color: #f97316;
		}}
		* {{ box-sizing: border-box; margin: 0; padding: 0; }}
		body {{
			background: var(--bg-main);
			color: var(--text-main);
			font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
			padding: 2rem;
			line-height: 1.5;
		}}
		header {{
			margin-bottom: 2rem;
			display: flex;
			flex-wrap: wrap;
			justify-content: space-between;
			align-items: center;
			border-bottom: 1px solid var(--border-color);
			padding-bottom: 1.5rem;
		}}
		h1 {{ font-size: 1.8rem; font-weight: 700; letter-spacing: -0.5px; }}
		.summary-table {{
			width: 100%;
			border-collapse: collapse;
			margin: 1.5rem 0 2rem;
			background: var(--bg-card);
			border: 1px solid var(--border-color);
			border-radius: 8px;
			overflow: hidden;
			font-size: 0.9rem;
		}}
		.summary-table th, .summary-table td {{
			padding: 0.5rem 0.9rem;
			text-align: left;
			border-bottom: 1px solid var(--border-color);
		}}
		.summary-table th {{
			background: var(--bg-card-header);
			font-size: 0.75rem;
			text-transform: uppercase;
			color: var(--text-muted);
		}}
		.summary-table tr:last-child td {{ border-bottom: none; }}
		.summary-table .num {{ text-align: right; font-variant-numeric: tabular-nums; }}
		.summary-table .pass {{ color: var(--pass-color); }}
		.summary-table .fail {{ color: var(--fail-color); }}
		.summary-table .skip {{ color: var(--skip-color); }}
		.summary-table .err {{ color: var(--error-color); }}
		.summary-table .rate {{ font-weight: 700; }}
		.summary-bar {{
			display: flex;
			gap: 1rem;
			margin: 1.5rem 0;
			flex-wrap: wrap;
		}}
		.summary-pill {{
			background: var(--bg-card);
			border: 1px solid var(--border-color);
			padding: 0.75rem 1.25rem;
			border-radius: 8px;
			display: flex;
			flex-direction: column;
			min-width: 120px;
		}}
		.summary-pill .val {{ font-size: 1.6rem; font-weight: bold; }}
		.summary-pill .lbl {{ font-size: 0.8rem; color: var(--text-muted); text-transform: uppercase; }}
		.pill-pass .val {{ color: var(--pass-color); }}
		.pill-fail .val {{ color: var(--fail-color); }}
		.pill-skip .val {{ color: var(--skip-color); }}
		.pill-err .val {{ color: var(--error-color); }}

		.controls {{
			display: flex;
			gap: 1rem;
			margin-bottom: 2rem;
			align-items: center;
			flex-wrap: wrap;
		}}
		.filter-btn {{
			background: var(--bg-card);
			border: 1px solid var(--border-color);
			color: var(--text-main);
			padding: 0.5rem 1rem;
			border-radius: 6px;
			cursor: pointer;
			font-size: 0.9rem;
			transition: all 0.2s;
		}}
		.filter-btn:hover, .filter-btn.active {{
			background: var(--accent-blue);
			border-color: var(--accent-blue);
		}}
		.search-input {{
			background: var(--bg-card);
			border: 1px solid var(--border-color);
			color: var(--text-main);
			padding: 0.5rem 1rem;
			border-radius: 6px;
			font-size: 0.9rem;
			min-width: 250px;
		}}
		.nav-btn {{
			background: var(--bg-card);
			border: 1px solid var(--border-color);
			color: var(--accent-blue);
			padding: 0.5rem 1rem;
			border-radius: 6px;
			text-decoration: none;
			font-size: 0.9rem;
			font-weight: 500;
			display: inline-block;
			transition: all 0.2s;
		}}
		.nav-btn:hover {{
			background: var(--accent-blue);
			color: #ffffff;
			border-color: var(--accent-blue);
		}}

		.test-card {{
			background: var(--bg-card);
			border: 1px solid var(--border-color);
			border-radius: 8px;
			margin-bottom: 1.5rem;
			overflow: hidden;
		}}
		.test-card.status-failed {{ border-left: 4px solid var(--fail-color); }}
		.test-card.status-passed {{ border-left: 4px solid var(--pass-color); }}
		.test-card.status-skipped {{ border-left: 4px solid var(--skip-color); }}
		.test-card.status-error {{ border-left: 4px solid var(--error-color); }}

		.card-header {{
			background: var(--bg-card-header);
			padding: 0.8rem 1.2rem;
			display: flex;
			justify-content: space-between;
			align-items: center;
		}}
		.test-name {{ font-weight: 600; font-family: monospace; font-size: 1rem; }}
		.badge {{
			padding: 0.2rem 0.6rem;
			border-radius: 4px;
			font-size: 0.75rem;
			font-weight: bold;
			text-transform: uppercase;
		}}
		.badge-passed {{ background: rgba(34, 197, 94, 0.2); color: var(--pass-color); border: 1px solid var(--pass-color); }}
		.badge-failed {{ background: rgba(239, 68, 68, 0.2); color: var(--fail-color); border: 1px solid var(--fail-color); }}
		.badge-skipped {{ background: rgba(234, 179, 8, 0.2); color: var(--skip-color); border: 1px solid var(--skip-color); }}
		.badge-error {{ background: rgba(249, 115, 22, 0.2); color: var(--error-color); border: 1px solid var(--error-color); }}

		.card-body {{ padding: 1.2rem; }}
		.metrics {{
			display: flex;
			gap: 1.5rem;
			margin-bottom: 1rem;
			font-size: 0.9rem;
			background: rgba(0,0,0,0.2);
			padding: 0.5rem 1rem;
			border-radius: 6px;
			flex-wrap: wrap;
		}}
		.error-msg {{ color: var(--error-color); margin-bottom: 1rem; font-family: monospace; font-size: 0.9rem; }}
		.size-mismatch {{ color: var(--error-color); font-weight: 600; width: 100%; }}

		.previews {{
			display: grid;
			grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
			gap: 1rem;
			margin-bottom: 1rem;
		}}
		.preview-col h4 {{ font-size: 0.85rem; color: var(--text-muted); margin-bottom: 0.5rem; text-transform: uppercase; }}
		.img-wrapper {{
			background: repeating-conic-gradient(#20222a 0% 25%, #2a2c36 0% 50%) 50% / 16px 16px;
			border: 1px solid var(--border-color);
			border-radius: 6px;
			padding: 0.5rem;
			display: flex;
			justify-content: center;
			align-items: center;
			min-height: 200px;
		}}
		.img-wrapper img {{
			max-width: 100%;
			height: auto;
			object-fit: contain;
			image-rendering: pixelated;
		}}
		.diff-bg {{ background: #141419; }}

		.code-details {{ margin-top: 1rem; }}
		.code-details summary {{ cursor: pointer; color: var(--accent-blue); font-size: 0.85rem; user-select: none; }}
		.code-split {{
			display: grid;
			grid-template-columns: 1fr 1fr;
			gap: 1rem;
			margin-top: 0.5rem;
		}}
		.code-split h5 {{ font-size: 0.75rem; color: var(--text-muted); margin-bottom: 0.25rem; }}
		pre {{
			background: #0d0e11;
			border: 1px solid var(--border-color);
			border-radius: 4px;
			padding: 0.75rem;
			overflow-x: auto;
			font-size: 0.8rem;
			max-height: 250px;
		}}
	</style>
</head>
<body>
	<header>
		<div>
			<h1>{title}</h1>
			<p style="color: var(--text-muted); font-size: 0.9rem; margin-top: 0.25rem;">SVG import correctness, cross-checked against resvg</p>
		</div>
		{nav_html}
	</header>

	{summary_table}

	<div class="summary-bar">
		<div class="summary-pill">
			<span class="val">{total}</span>
			<span class="lbl">Total Tests</span>
		</div>
		<div class="summary-pill pill-pass">
			<span class="val">{passed}</span>
			<span class="lbl">Passed</span>
		</div>
		<div class="summary-pill pill-fail">
			<span class="val">{failed}</span>
			<span class="lbl">Failed</span>
		</div>
		<div class="summary-pill pill-skip">
			<span class="val">{skipped}</span>
			<span class="lbl">Skipped</span>
		</div>
		<div class="summary-pill pill-err">
			<span class="val">{errors}</span>
			<span class="lbl">Errors</span>
		</div>
	</div>

	<div class="controls">
		<button class="filter-btn active" data-filter="all" onclick="filterStatus('all')">All ({total})</button>
		<button class="filter-btn" data-filter="failed" onclick="filterStatus('failed')">Failed ({failed})</button>
		<button class="filter-btn" data-filter="passed" onclick="filterStatus('passed')">Passed ({passed})</button>
		<button class="filter-btn" data-filter="error" onclick="filterStatus('error')">Errors ({errors})</button>
		<button class="filter-btn" data-filter="skipped" onclick="filterStatus('skipped')">Skipped ({skipped})</button>
		<input type="text" class="search-input" placeholder="Search test name..." oninput="searchTests(this.value)" />
	</div>

	<div id="test-list">
		{test_cards}
	</div>

	<script>
		// The card list is read once at load. With a full corpus this is thousands of nodes, and
		// re-querying the DOM on every keystroke makes search feel sluggish.
		const cards = Array.from(document.querySelectorAll('.test-card'), card => ({{
			el: card,
			status: card.dataset.status,
			name: (card.dataset.name || '').toLowerCase(),
		}}));
		const filterButtons = Array.from(document.querySelectorAll('.filter-btn'));
		let currentFilter = 'all';
		let currentQuery = '';

		function applyFilters() {{
			for (const card of cards) {{
				const matchesStatus = currentFilter === 'all' || card.status === currentFilter;
				const matchesQuery = currentQuery === '' || card.name.includes(currentQuery);
				card.el.hidden = !(matchesStatus && matchesQuery);
			}}
		}}

		function filterStatus(status) {{
			currentFilter = status;
			for (const btn of filterButtons) {{
				btn.classList.toggle('active', btn.dataset.filter === status);
			}}
			applyFilters();
		}}

		function searchTests(query) {{
			currentQuery = query.toLowerCase().trim();
			applyFilters();
		}}
	</script>
</body>
</html>"##,
		title = html_escape(title),
		total = total,
		passed = passed,
		failed = failed,
		skipped = skipped,
		errors = errors,
		test_cards = test_cards,
	)
}

fn html_escape(s: &str) -> String {
	s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}
