use crate::messages::frontend::utility_types::{ExportBounds, FileType};
use crate::messages::prelude::*;
use glam::{DAffine2, DVec2, UVec2};
use graph_craft::document::value::{RenderOutput, TaggedValue};
use graph_craft::document::{DocumentNode, DocumentNodeImplementation, NodeId, NodeInput};
use graph_craft::proto::GraphErrors;
use graph_craft::wasm_application_io::EditorPreferences;
use graphene_std::application_io::{NodeGraphUpdateMessage, RenderConfig};
use graphene_std::application_io::{SurfaceFrame, TimingInformation};
use graphene_std::renderer::{RenderMetadata, format_transform_matrix};
use graphene_std::text::FontCache;
use graphene_std::transform::Footprint;
use graphene_std::vector::Vector;
use graphene_std::wasm_application_io::RenderOutputType;
use interpreted_executor::dynamic_executor::ResolvedDocumentNodeTypesDelta;

mod runtime_io;
pub use runtime_io::NodeRuntimeIO;

mod runtime;
pub use runtime::*;

#[cfg(feature = "gpu")]
fn image_to_ascii_svg(image: &image::RgbaImage) -> (String, (f64, f64)) {
	const ASCII_CHARS: &[u8] = b" .,:;i1tfLCG08@";
	const CHAR_WIDTH: f32 = 6.0;
	const CHAR_HEIGHT: f32 = 10.0;
	const CONTRAST_FACTOR: f32 = 1.2;
	const CELL_SIZE: u32 = 8; // Group 8x8 pixels into one character for reasonable output size

	let (img_width, img_height) = image.dimensions();
	let ascii_cols = (img_width / CELL_SIZE).max(1);
	let ascii_rows = (img_height / CELL_SIZE).max(1);
	let svg_width = ascii_cols as f32 * CHAR_WIDTH;
	let svg_height = ascii_rows as f32 * CHAR_HEIGHT;

	let get_luminance = |pixel: &image::Rgba<u8>| {
		let [r, g, b, _a] = pixel.0;
		(0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.0
	};

	let contrast = |lum: f32| -> f32 { ((lum - 0.5) * CONTRAST_FACTOR + 0.5).clamp(0.0, 1.0) };

	// Sample a cell and return average luminance and average color
	let sample_cell = |cell_x: u32, cell_y: u32| -> (f32, [u8; 3]) {
		let start_x = cell_x * CELL_SIZE;
		let start_y = cell_y * CELL_SIZE;
		let end_x = (start_x + CELL_SIZE).min(img_width);
		let end_y = (start_y + CELL_SIZE).min(img_height);

		let mut total_lum = 0.0f32;
		let mut total_r = 0u32;
		let mut total_g = 0u32;
		let mut total_b = 0u32;
		let mut count = 0u32;

		for y in start_y..end_y {
			for x in start_x..end_x {
				let pixel = image.get_pixel(x, y);
				total_lum += get_luminance(pixel);
				total_r += pixel.0[0] as u32;
				total_g += pixel.0[1] as u32;
				total_b += pixel.0[2] as u32;
				count += 1;
			}
		}

		if count == 0 {
			return (0.0, [0, 0, 0]);
		}

		let avg_lum = contrast(total_lum / count as f32);
		let avg_color = [(total_r / count) as u8, (total_g / count) as u8, (total_b / count) as u8];
		(avg_lum, avg_color)
	};

	let mut svg = String::with_capacity((ascii_cols as usize + 1) * ascii_rows as usize * 50);
	svg.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>"#);
	svg.push_str(&format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}">"#, svg_width, svg_height));
	svg.push_str(r#"<style>text{font-family:'Courier New',Courier,monospace;font-size:10px;white-space:pre;}</style>"#);
	svg.push_str(r#"<rect width="100%" height="100%" fill="black" />"#);

	for row in 0..ascii_rows {
		let y_pos = (row as f32 + 1.0) * CHAR_HEIGHT;
		svg.push_str(&format!(r#"<text x="0" y="{}" xml:space="preserve">"#, y_pos));
		for col in 0..ascii_cols {
			let (lum, color) = sample_cell(col, row);
			let color_hex = format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2]);

			// Map luminance to character index
			let char_idx = ((lum * (ASCII_CHARS.len() - 1) as f32).round() as usize).min(ASCII_CHARS.len() - 1);
			let ascii_char = ASCII_CHARS[char_idx] as char;

			if ascii_char == ' ' {
				svg.push(' ');
			} else {
				let content = match ascii_char {
					'<' => "&lt;".to_string(),
					'>' => "&gt;".to_string(),
					'&' => "&amp;".to_string(),
					'"' => "&quot;".to_string(),
					c => c.to_string(),
				};
				svg.push_str(&format!(r#"<tspan fill="{}">{}</tspan>"#, color_hex, content));
			}
		}
		svg.push_str("</text>");
	}

	svg.push_str("</svg>");
	(svg, (svg_width as f64, svg_height as f64))
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ExecutionRequest {
	execution_id: u64,
	render_config: RenderConfig,
	export_file_type: Option<FileType>,
}

pub struct ExecutionResponse {
	execution_id: u64,
	result: Result<TaggedValue, String>,
	responses: VecDeque<FrontendMessage>,
	vector_modify: HashMap<NodeId, Vector>,
	/// The resulting value from the temporary inspected during execution
	inspect_result: Option<InspectResult>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CompilationResponse {
	result: Result<ResolvedDocumentNodeTypesDelta, (ResolvedDocumentNodeTypesDelta, String)>,
	node_graph_errors: GraphErrors,
}

pub enum NodeGraphUpdate {
	ExecutionResponse(ExecutionResponse),
	CompilationResponse(CompilationResponse),
	NodeGraphUpdateMessage(NodeGraphUpdateMessage),
}

#[derive(Debug, Default)]
pub struct NodeGraphExecutor {
	runtime_io: NodeRuntimeIO,
	current_execution_id: u64,
	futures: VecDeque<(u64, ExecutionContext)>,
	node_graph_hash: u64,
	previous_node_to_inspect: Option<NodeId>,
	last_svg_canvas: Option<SurfaceFrame>,
}

#[derive(Debug, Clone)]
struct ExecutionContext {
	render_config: RenderConfig,
	export_config: Option<ExportConfig>,
	document_id: DocumentId,
}

impl NodeGraphExecutor {
	/// A local runtime is useful on threads since having global state causes flakes
	#[cfg(test)]
	pub(crate) fn new_with_local_runtime() -> (NodeRuntime, Self) {
		let (request_sender, request_receiver) = std::sync::mpsc::channel();
		let (response_sender, response_receiver) = std::sync::mpsc::channel();
		let node_runtime = NodeRuntime::new(request_receiver, response_sender);

		let node_executor = Self {
			futures: Default::default(),
			runtime_io: NodeRuntimeIO::with_channels(request_sender, response_receiver),
			node_graph_hash: 0,
			current_execution_id: 0,
			previous_node_to_inspect: None,
			last_svg_canvas: None,
		};
		(node_runtime, node_executor)
	}

	/// Execute the network by flattening it and creating a borrow stack.
	fn queue_execution(&mut self, render_config: RenderConfig, export_file_type: Option<FileType>) -> u64 {
		let execution_id = self.current_execution_id;
		self.current_execution_id += 1;
		let request = ExecutionRequest {
			execution_id,
			render_config,
			export_file_type,
		};
		self.runtime_io.send(GraphRuntimeRequest::ExecutionRequest(request)).expect("Failed to send generation request");

		execution_id
	}

	pub fn update_font_cache(&self, font_cache: FontCache) {
		self.runtime_io.send(GraphRuntimeRequest::FontCacheUpdate(font_cache)).expect("Failed to send font cache update");
	}

	pub fn update_editor_preferences(&self, editor_preferences: EditorPreferences) {
		self.runtime_io
			.send(GraphRuntimeRequest::EditorPreferencesUpdate(editor_preferences))
			.expect("Failed to send editor preferences");
	}

	/// Updates the network to monitor all inputs. Useful for the testing.
	#[cfg(test)]
	pub(crate) fn update_node_graph_instrumented(&mut self, document: &mut DocumentMessageHandler) -> Result<Instrumented, String> {
		// We should always invalidate the cache.
		self.node_graph_hash = crate::application::generate_uuid();
		let mut network = document.network_interface.document_network().clone();
		let instrumented = Instrumented::new(&mut network);

		self.runtime_io
			.send(GraphRuntimeRequest::GraphUpdate(GraphUpdate { network, node_to_inspect: None }))
			.map_err(|e| e.to_string())?;
		Ok(instrumented)
	}

	/// Update the cached network if necessary.
	fn update_node_graph(&mut self, document: &mut DocumentMessageHandler, node_to_inspect: Option<NodeId>, ignore_hash: bool) -> Result<(), String> {
		let network_hash = document.network_interface.network_hash();
		// Refresh the graph when it changes or the inspect node changes
		if network_hash != self.node_graph_hash || self.previous_node_to_inspect != node_to_inspect || ignore_hash {
			let network = document.network_interface.document_network().clone();
			self.previous_node_to_inspect = node_to_inspect;
			self.node_graph_hash = network_hash;

			self.runtime_io
				.send(GraphRuntimeRequest::GraphUpdate(GraphUpdate { network, node_to_inspect }))
				.map_err(|e| e.to_string())?;
		}

		Ok(())
	}

	/// Adds an evaluate request for whatever current network is cached.
	pub(crate) fn submit_current_node_graph_evaluation(
		&mut self,
		document: &mut DocumentMessageHandler,
		document_id: DocumentId,
		viewport_resolution: UVec2,
		viewport_scale: f64,
		time: TimingInformation,
		pointer: DVec2,
	) -> Result<Message, String> {
		let viewport = Footprint {
			transform: document.metadata().document_to_viewport,
			resolution: viewport_resolution,
			..Default::default()
		};
		let render_config = RenderConfig {
			viewport,
			scale: viewport_scale,
			time,
			pointer,
			export_format: graphene_std::application_io::ExportFormat::Raster,
			render_mode: document.render_mode,
			hide_artboards: false,
			for_export: false,
			for_eyedropper: false,
		};

		// Execute the node graph
		let execution_id = self.queue_execution(render_config, None);

		self.futures.push_back((
			execution_id,
			ExecutionContext {
				render_config,
				export_config: None,
				document_id,
			},
		));

		Ok(DeferMessage::SetGraphSubmissionIndex { execution_id }.into())
	}

	/// Evaluates a node graph, computing the entire graph
	#[allow(clippy::too_many_arguments)]
	pub fn submit_node_graph_evaluation(
		&mut self,
		document: &mut DocumentMessageHandler,
		document_id: DocumentId,
		viewport_resolution: UVec2,
		viewport_scale: f64,
		time: TimingInformation,
		node_to_inspect: Option<NodeId>,
		ignore_hash: bool,
		pointer: DVec2,
	) -> Result<Message, String> {
		self.update_node_graph(document, node_to_inspect, ignore_hash)?;
		self.submit_current_node_graph_evaluation(document, document_id, viewport_resolution, viewport_scale, time, pointer)
	}

	#[cfg(not(target_family = "wasm"))]
	pub(crate) fn submit_eyedropper_preview(&mut self, document_id: DocumentId, transform: DAffine2, pointer: DVec2, resolution: UVec2, time: TimingInformation) -> Result<Message, String> {
		let viewport = Footprint {
			transform,
			resolution,
			..Default::default()
		};
		let render_config = RenderConfig {
			viewport,
			scale: 1.,
			time,
			pointer,
			export_format: graphene_std::application_io::ExportFormat::Raster,
			render_mode: graphene_std::vector::style::RenderMode::Normal,
			hide_artboards: false,
			for_export: false,
			for_eyedropper: true,
		};

		// Execute the node graph
		let execution_id = self.queue_execution(render_config, None);

		self.futures.push_back((
			execution_id,
			ExecutionContext {
				render_config,
				export_config: None,
				document_id,
			},
		));

		Ok(DeferMessage::SetGraphSubmissionIndex { execution_id }.into())
	}

	/// Evaluates a node graph for export
	pub fn submit_document_export(&mut self, document: &mut DocumentMessageHandler, document_id: DocumentId, mut export_config: ExportConfig) -> Result<(), String> {
		let network = document.network_interface.document_network().clone();

		let export_format = if export_config.file_type == FileType::Svg {
			graphene_std::application_io::ExportFormat::Svg
		} else {
			graphene_std::application_io::ExportFormat::Raster
		};

		// Calculate the bounding box of the region to be exported
		let bounds = match export_config.bounds {
			ExportBounds::AllArtwork => document.network_interface.document_bounds_document_space(!export_config.transparent_background),
			ExportBounds::Selection => document.network_interface.selected_bounds_document_space(!export_config.transparent_background, &[]),
			ExportBounds::Artboard(id) => document.metadata().bounding_box_document(id),
		}
		.ok_or_else(|| "No bounding box".to_string())?;
		let resolution = (bounds[1] - bounds[0]).round().as_uvec2();
		let transform = DAffine2::from_translation(bounds[0]).inverse();

		let render_config = RenderConfig {
			viewport: Footprint {
				resolution,
				transform,
				..Default::default()
			},
			scale: export_config.scale_factor,
			time: Default::default(),
			pointer: DVec2::ZERO,
			export_format,
			render_mode: document.render_mode,
			hide_artboards: export_config.transparent_background,
			for_export: true,
			for_eyedropper: false,
		};
		export_config.size = resolution.as_dvec2();

		// Execute the node graph
		self.runtime_io
			.send(GraphRuntimeRequest::GraphUpdate(GraphUpdate { network, node_to_inspect: None }))
			.map_err(|e| e.to_string())?;
		let execution_id = self.queue_execution(render_config, Some(export_config.file_type));
		self.futures.push_back((
			execution_id,
			ExecutionContext {
				render_config,
				export_config: Some(export_config),
				document_id,
			},
		));

		Ok(())
	}

	pub fn poll_node_graph_evaluation(&mut self, document: &mut DocumentMessageHandler, responses: &mut VecDeque<Message>) -> Result<(), String> {
		let results = self.runtime_io.receive().collect::<Vec<_>>();
		for response in results {
			match response {
				NodeGraphUpdate::ExecutionResponse(execution_response) => {
					let ExecutionResponse {
						execution_id,
						result,
						responses: existing_responses,
						vector_modify,
						inspect_result,
					} = execution_response;

					responses.add(OverlaysMessage::Draw);

					let node_graph_output = match result {
						Ok(output) => output,
						Err(e) => {
							// Clear the click targets while the graph is in an un-renderable state
							document.network_interface.update_click_targets(HashMap::new());
							document.network_interface.update_vector_modify(HashMap::new());
							return Err(format!("Node graph evaluation failed:\n{e}"));
						}
					};

					responses.extend(existing_responses.into_iter().map(Into::into));
					document.network_interface.update_vector_modify(vector_modify);

					while let Some(&(fid, _)) = self.futures.front() {
						if fid < execution_id {
							self.futures.pop_front();
						} else {
							break;
						}
					}

					let Some((fid, execution_context)) = self.futures.pop_front() else {
						panic!("InvalidGenerationId")
					};
					assert_eq!(fid, execution_id, "Missmatch in execution id");

					if let Some(export_config) = execution_context.export_config {
						// Special handling for exporting the artwork
						self.process_export(node_graph_output, export_config, responses)?;
					} else if execution_context.render_config.for_eyedropper {
						// Special handling for Eyedropper tool preview
						self.process_eyedropper_preview(node_graph_output, responses)?;
					} else {
						self.process_node_graph_output(node_graph_output, responses)?;
					}
					responses.add(DeferMessage::TriggerGraphRun {
						execution_id,
						document_id: execution_context.document_id,
					});

					// Update the Data panel on the frontend using the value of the inspect result.
					if let Some(inspect_result) = (self.previous_node_to_inspect.is_some()).then_some(inspect_result).flatten() {
						responses.add(DataPanelMessage::UpdateLayout { inspect_result });
					} else {
						responses.add(DataPanelMessage::ClearLayout);
					}
				}
				NodeGraphUpdate::CompilationResponse(execution_response) => {
					let CompilationResponse { node_graph_errors, result } = execution_response;
					let type_delta = match result {
						Err((incomplete_delta, e)) => {
							// Clear the click targets while the graph is in an un-renderable state

							document.network_interface.update_click_targets(HashMap::new());
							document.network_interface.update_vector_modify(HashMap::new());

							log::trace!("{e}");

							responses.add(NodeGraphMessage::UpdateTypes {
								resolved_types: incomplete_delta,
								node_graph_errors,
							});
							responses.add(NodeGraphMessage::SendGraph);

							return Err(format!("Node graph evaluation failed:\n{e}"));
						}
						Ok(result) => result,
					};

					responses.add(NodeGraphMessage::UpdateTypes {
						resolved_types: type_delta,
						node_graph_errors,
					});
					responses.add(NodeGraphMessage::SendGraph);
				}
			}
		}

		Ok(())
	}

	fn process_node_graph_output(&mut self, node_graph_output: TaggedValue, responses: &mut VecDeque<Message>) -> Result<(), String> {
		let TaggedValue::RenderOutput(render_output) = node_graph_output else {
			return Err(format!("Invalid node graph output type: {node_graph_output:#?}"));
		};

		match render_output.data {
			RenderOutputType::Svg { svg, image_data } => {
				// Send to frontend
				responses.add(FrontendMessage::UpdateImageData { image_data });
				responses.add(FrontendMessage::UpdateDocumentArtwork { svg });
				self.last_svg_canvas = None;
			}
			RenderOutputType::CanvasFrame(frame) => 'block: {
				if self.last_svg_canvas == Some(frame) {
					break 'block;
				}
				let matrix = format_transform_matrix(frame.transform);
				let transform = if matrix.is_empty() { String::new() } else { format!(" transform=\"{matrix}\"") };
				let svg = format!(
					r#"<svg><foreignObject width="{}" height="{}"{transform}><div data-canvas-placeholder="{}" data-is-viewport="true"></div></foreignObject></svg>"#,
					frame.resolution.x, frame.resolution.y, frame.surface_id.0,
				);
				self.last_svg_canvas = Some(frame);
				responses.add(FrontendMessage::UpdateDocumentArtwork { svg });
			}
			RenderOutputType::Texture { .. } => {}
			_ => return Err(format!("Invalid node graph output type: {:#?}", render_output.data)),
		}

		let RenderMetadata {
			upstream_footprints,
			local_transforms,
			first_element_source_id,
			click_targets,
			clip_targets,
		} = render_output.metadata;

		// Run these update state messages immediately
		responses.add(DocumentMessage::UpdateUpstreamTransforms {
			upstream_footprints,
			local_transforms,
			first_element_source_id,
		});
		responses.add(DocumentMessage::UpdateClickTargets { click_targets });
		responses.add(DocumentMessage::UpdateClipTargets { clip_targets });
		responses.add(DocumentMessage::RenderScrollbars);
		responses.add(DocumentMessage::RenderRulers);
		responses.add(OverlaysMessage::Draw);

		Ok(())
	}

	fn process_eyedropper_preview(&self, node_graph_output: TaggedValue, responses: &mut VecDeque<Message>) -> Result<(), String> {
		match node_graph_output {
			#[cfg(feature = "gpu")]
			TaggedValue::RenderOutput(RenderOutput {
				data: RenderOutputType::Buffer { data, width, height },
				..
			}) => {
				responses.add(EyedropperToolMessage::PreviewImage { data, width, height });
			}
			_ => {
				// TODO: Support Eyedropper preview in SVG mode on desktop
			}
		};

		Ok(())
	}

	fn process_export(&self, node_graph_output: TaggedValue, export_config: ExportConfig, responses: &mut VecDeque<Message>) -> Result<(), String> {
		let ExportConfig {
			file_type,
			name,
			size,
			scale_factor,
			#[cfg(feature = "gpu")]
			transparent_background,
			artboard_name,
			artboard_count,
			..
		} = export_config;

		let file_extension = match file_type {
			FileType::Svg => "svg",
			FileType::Png => "png",
			FileType::Jpg => "jpg",
			FileType::Ascii => "svg",
		};
		let base_name = match (artboard_name, artboard_count) {
			(Some(artboard_name), count) if count > 1 => format!("{name} - {artboard_name}"),
			_ => name,
		};
		let name = format!("{base_name}.{file_extension}");

		match node_graph_output {
			TaggedValue::RenderOutput(RenderOutput {
				data: RenderOutputType::Svg { svg, .. },
				..
			}) => {
				if file_type == FileType::Svg {
					responses.add(FrontendMessage::TriggerSaveFile { name, content: svg.into_bytes() });
				} else if file_type == FileType::Ascii {
					return Err("ASCII export requires raster output. Please ensure your document contains raster content or use PNG/JPG export.".to_string());
				} else {
					let mime = file_type.to_mime().to_string();
					let size = (size * scale_factor).into();
					responses.add(FrontendMessage::TriggerExportImage { svg, name, mime, size });
				}
			}
			#[cfg(feature = "gpu")]
			TaggedValue::RenderOutput(RenderOutput {
				data: RenderOutputType::Buffer { data, width, height },
				..
			}) if file_type != FileType::Svg => {
				use image::buffer::ConvertBuffer;
				use image::{ImageFormat, RgbImage, RgbaImage};

				let Some(image) = RgbaImage::from_raw(width, height, data) else {
					return Err("Failed to create image buffer for export".to_string());
				};

				let mut encoded = Vec::new();
				let mut cursor = std::io::Cursor::new(&mut encoded);

				match file_type {
					FileType::Png => {
						let result = if transparent_background {
							image.write_to(&mut cursor, ImageFormat::Png)
						} else {
							let image: RgbImage = image.convert();
							image.write_to(&mut cursor, ImageFormat::Png)
						};
						if let Err(err) = result {
							return Err(format!("Failed to encode PNG: {err}"));
						}
					}
					FileType::Jpg => {
						let image: RgbImage = image.convert();
						let result = image.write_to(&mut cursor, ImageFormat::Jpeg);
						if let Err(err) = result {
							return Err(format!("Failed to encode JPG: {err}"));
						}
					}
					FileType::Svg => {
						return Err("SVG cannot be exported from an image buffer".to_string());
					}
					FileType::Ascii => {
						let (ascii_svg, _size) = image_to_ascii_svg(&image);
						return Ok(responses.add(FrontendMessage::TriggerSaveFile {
							name,
							content: ascii_svg.into_bytes(),
						}));
					}
				}

				responses.add(FrontendMessage::TriggerSaveFile { name, content: encoded });
			}
			_ => {
				return Err(format!("Incorrect render type for exporting to an SVG ({file_type:?}, {node_graph_output})"));
			}
		};

		Ok(())
	}
}

// Re-export for usage by tests in other modules
#[cfg(test)]
pub use test::Instrumented;

#[cfg(test)]
mod test {
	use std::sync::Arc;

	use super::*;
	use crate::messages::portfolio::document::utility_types::network_interface::NodeNetworkInterface;
	use crate::test_utils::test_prelude::{self, NodeGraphLayer};
	use graph_craft::ProtoNodeIdentifier;
	use graph_craft::document::NodeNetwork;
	use graphene_std::Context;
	use graphene_std::NodeInputDecleration;
	use graphene_std::memo::IORecord;
	use test_prelude::LayerNodeIdentifier;

	/// Stores all of the monitor nodes that have been attached to a graph
	#[derive(Default)]
	pub struct Instrumented {
		protonodes_by_name: HashMap<ProtoNodeIdentifier, Vec<Vec<Vec<NodeId>>>>,
		protonodes_by_path: HashMap<Vec<NodeId>, Vec<Vec<NodeId>>>,
	}

	impl Instrumented {
		/// Adds montior nodes to the network
		fn add(&mut self, network: &mut NodeNetwork, path: &mut Vec<NodeId>) {
			// Required to do seperately to satiate the borrow checker.
			let mut monitor_nodes = Vec::new();
			for (id, node) in network.nodes.iter_mut() {
				// Recursively instrument
				if let DocumentNodeImplementation::Network(nested) = &mut node.implementation {
					path.push(*id);
					self.add(nested, path);
					path.pop();
				}
				let mut monitor_node_ids = Vec::with_capacity(node.inputs.len());
				for input in &mut node.inputs {
					let node_id = NodeId::new();
					let old_input = std::mem::replace(input, NodeInput::node(node_id, 0));
					monitor_nodes.push((old_input, node_id));
					path.push(node_id);
					monitor_node_ids.push(path.clone());
					path.pop();
				}
				if let DocumentNodeImplementation::ProtoNode(identifier) = &mut node.implementation {
					path.push(*id);
					self.protonodes_by_name.entry(identifier.clone()).or_default().push(monitor_node_ids.clone());
					self.protonodes_by_path.insert(path.clone(), monitor_node_ids);
					path.pop();
				}
			}
			for (input, monitor_id) in monitor_nodes {
				let monitor_node = DocumentNode {
					inputs: vec![input],
					implementation: DocumentNodeImplementation::ProtoNode(graphene_std::memo::monitor::IDENTIFIER),
					call_argument: graph_craft::generic!(T),
					skip_deduplication: true,
					..Default::default()
				};
				network.nodes.insert(monitor_id, monitor_node);
			}
		}

		/// Instrument a graph and return a new [Instrumented] state.
		pub fn new(network: &mut NodeNetwork) -> Self {
			let mut instrumented = Self::default();
			instrumented.add(network, &mut Vec::new());
			instrumented
		}

		fn downcast<Input: NodeInputDecleration>(dynamic: Arc<dyn std::any::Any + Send + Sync>) -> Option<Input::Result>
		where
			Input::Result: Send + Sync + Clone + 'static,
		{
			// This is quite inflexible since it only allows the footprint as inputs.
			if let Some(x) = dynamic.downcast_ref::<IORecord<(), Input::Result>>() {
				Some(x.output.clone())
			} else if let Some(x) = dynamic.downcast_ref::<IORecord<Footprint, Input::Result>>() {
				Some(x.output.clone())
			} else if let Some(x) = dynamic.downcast_ref::<IORecord<Context, Input::Result>>() {
				Some(x.output.clone())
			} else {
				warn!("cannot downcast type for introspection");
				None
			}
		}

		/// Grab all of the values of the input every time it occurs in the graph.
		pub fn grab_all_input<'a, Input: NodeInputDecleration + 'a>(&'a self, runtime: &'a NodeRuntime) -> impl Iterator<Item = Input::Result> + 'a
		where
			Input::Result: Send + Sync + Clone + 'static,
		{
			self.protonodes_by_name
				.get(&Input::identifier())
				.map_or([].as_slice(), |x| x.as_slice())
				.iter()
				.filter_map(|inputs| inputs.get(Input::INDEX))
				.filter_map(|input_monitor_node| runtime.executor.introspect(input_monitor_node).ok())
				.filter_map(Instrumented::downcast::<Input>) // Some might not resolve (e.g. generics that don't work properly)
		}

		pub fn grab_protonode_input<Input: NodeInputDecleration>(&self, path: &Vec<NodeId>, runtime: &NodeRuntime) -> Option<Input::Result>
		where
			Input::Result: Send + Sync + Clone + 'static,
		{
			let input_monitor_node = self.protonodes_by_path.get(path)?.get(Input::INDEX)?;

			let dynamic = runtime.executor.introspect(input_monitor_node).ok()?;

			Self::downcast::<Input>(dynamic)
		}

		pub fn grab_input_from_layer<Input: NodeInputDecleration>(&self, layer: LayerNodeIdentifier, network_interface: &NodeNetworkInterface, runtime: &NodeRuntime) -> Option<Input::Result>
		where
			Input::Result: Send + Sync + Clone + 'static,
		{
			let node_graph_layer = NodeGraphLayer::new(layer, network_interface);
			let node = node_graph_layer.upstream_node_id_from_protonode(Input::identifier())?;
			self.grab_protonode_input::<Input>(&vec![node], runtime)
		}
	}
}
