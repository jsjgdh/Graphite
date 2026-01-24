use super::tool_prelude::*;
use crate::messages::portfolio::document::overlays::utility_types::OverlayContext;
use crate::messages::portfolio::document::utility_types::document_metadata::LayerNodeIdentifier;
use crate::messages::tool::common_functionality::auto_panning::AutoPanning;
use graph_craft::document::NodeInput;
use graph_craft::document::value::TaggedValue;
use graphene_std::vector::PointId;

#[derive(Default, ExtractField)]
pub struct TorchTool {
	fsm_state: TorchToolFsmState,
	data: TorchToolData,
}

use crate::messages::tool::common_functionality::graph_modification_utils::NodeGraphLayer;
use graph_craft::document::NodeId;

#[impl_message(Message, ToolMessage, Torch)]
#[derive(PartialEq, Clone, Debug, Hash, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum TorchToolMessage {
	// Standard messages
	Abort,
	Overlays { context: OverlayContext },

	// Tool-specific messages
	PointerDown,
	PointerMove,
	PointerUp,
}

impl ToolMetadata for TorchTool {
	fn icon_name(&self) -> String {
		"GeneralTorchTool".into()
	}
	fn tooltip_label(&self) -> String {
		"Torch Tool".into()
	}
	fn tool_type(&self) -> crate::messages::tool::utility_types::ToolType {
		ToolType::Torch
	}
}

#[message_handler_data]
impl<'a> MessageHandler<ToolMessage, &mut ToolActionMessageContext<'a>> for TorchTool {
	fn process_message(&mut self, message: ToolMessage, responses: &mut VecDeque<Message>, context: &mut ToolActionMessageContext<'a>) {
		if message == ToolMessage::UpdateHints {
			self.fsm_state.update_hints(responses);
			return;
		}
		if message == ToolMessage::UpdateCursor {
			self.fsm_state.update_cursor(responses);
			return;
		}
		self.fsm_state.process_event(message, &mut self.data, context, &(), responses, false);
	}

	advertise_actions!(TorchToolMessageDiscriminant;
		PointerDown,
		PointerUp,
		PointerMove,
		Abort,
	);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TorchToolFsmState {
	#[default]
	Ready,
	Dragging,
}

#[derive(Clone, Debug, Default)]
struct TorchToolData {
	drag_start: DVec2,
	auto_panning: AutoPanning,
	selected_node: Option<(LayerNodeIdentifier, NodeId, usize)>, // Layer, NodeId, InputIndex
}

impl Fsm for TorchToolFsmState {
	type ToolData = TorchToolData;
	type ToolOptions = ();

	fn transition(
		self,
		event: ToolMessage,
		tool_data: &mut Self::ToolData,
		tool_action_data: &mut ToolActionMessageContext,
		_tool_options: &Self::ToolOptions,
		responses: &mut VecDeque<Message>,
	) -> Self {
		let ToolActionMessageContext { document, input, viewport, .. } = tool_action_data;

		let ToolMessage::Torch(event) = event else { return self };

		match (self, event) {
			(_, TorchToolMessage::Overlays { context: mut overlay_context }) => {
				for layer in document.network_interface.selected_nodes().selected_visible_layers(&document.network_interface) {
					let node_graph_layer = NodeGraphLayer::new(layer, &document.network_interface);
					let transform = document.metadata().transform_to_viewport(layer);

					for node_id in node_graph_layer.horizontal_layer_flow() {
						let Some(node) = document.network_interface.document_node(&node_id, &[]) else { continue };

						let mut is_drop_shadow = false;
						let mut light_pos = DVec2::ZERO;
						let mut use_light = false;

						for (index, _) in node.inputs.iter().enumerate() {
							let (name, _) = document.network_interface.displayed_input_name_and_description(&node_id, index, &[]);
							if name == "Use Light Source" {
								if let Some(TaggedValue::Bool(true)) = node.inputs.get(index).and_then(|i| i.as_value()) {
									use_light = true;
									is_drop_shadow = true;
								}
							}
							if name == "Light Position" {
								if let Some(TaggedValue::DVec2(val)) = node.inputs.get(index).and_then(|i| i.as_value()) {
									light_pos = *val;
								}
							}
						}

						if is_drop_shadow && use_light {
							let world_pos = transform.transform_point2(light_pos);

							// Draw Torch Icon/Handle
							overlay_context.manipulator_handle(world_pos, false, None);

							// Draw line to origin
							let origin = transform.transform_point2(DVec2::ZERO);
							overlay_context.line(origin, world_pos, None, None);
						}
					}
				}
				self
			}
			(TorchToolFsmState::Ready, TorchToolMessage::PointerDown) => {
				tool_data.drag_start = input.mouse.position;
				tool_data.selected_node = None;

				for layer in document.network_interface.selected_nodes().selected_visible_layers(&document.network_interface) {
					let node_graph_layer = NodeGraphLayer::new(layer, &document.network_interface);
					let transform = document.metadata().transform_to_viewport(layer);

					for node_id in node_graph_layer.horizontal_layer_flow() {
						let Some(node) = document.network_interface.document_node(&node_id, &[]) else { continue };

						let mut is_drop_shadow = false;
						let mut light_pos = DVec2::ZERO;
						let mut light_pos_index = 0;

						for (index, _) in node.inputs.iter().enumerate() {
							let (name, _) = document.network_interface.displayed_input_name_and_description(&node_id, index, &[]);
							if name == "Use Light Source" {
								if let Some(TaggedValue::Bool(true)) = node.inputs.get(index).and_then(|i| i.as_value()) {
									is_drop_shadow = true;
								}
							}
							if name == "Light Position" {
								light_pos_index = index;
								if let Some(TaggedValue::DVec2(val)) = node.inputs.get(index).and_then(|i| i.as_value()) {
									light_pos = *val;
								}
							}
						}

						if is_drop_shadow {
							let world_pos = transform.transform_point2(light_pos);
							if world_pos.distance_squared(input.mouse.position) < 400.0 {
								// 20px radius
								tool_data.selected_node = Some((layer, node_id, light_pos_index));
								return TorchToolFsmState::Dragging;
							}
						}
					}
				}
				TorchToolFsmState::Ready
			}
			(TorchToolFsmState::Dragging, TorchToolMessage::PointerMove) => {
				if let Some((layer, node_id, input_index)) = tool_data.selected_node {
					let transform = document.metadata().transform_to_viewport(layer);
					let local_mouse = transform.inverse().transform_point2(input.mouse.position);

					responses.add(NodeGraphMessage::SetInputValue {
						node_id,
						input_index,
						value: TaggedValue::DVec2(local_mouse),
					});
				}
				TorchToolFsmState::Dragging
			}
			(TorchToolFsmState::Dragging, TorchToolMessage::PointerUp) => {
				tool_data.selected_node = None;
				TorchToolFsmState::Ready
			}
			(_, TorchToolMessage::Abort) => {
				tool_data.selected_node = None;
				TorchToolFsmState::Ready
			}
			_ => self,
		}
	}

	fn update_hints(&self, responses: &mut VecDeque<Message>) {
		let hint_data = match self {
			TorchToolFsmState::Ready => HintData(vec![HintGroup(vec![HintInfo::mouse(MouseMotion::LmbDrag, "Drag Light")])]),
			TorchToolFsmState::Dragging => HintData(vec![HintGroup(vec![HintInfo::mouse(MouseMotion::Rmb, ""), HintInfo::keys([Key::Escape], "Cancel").prepend_slash()])]),
		};

		hint_data.send_layout(responses);
	}

	fn update_cursor(&self, responses: &mut VecDeque<Message>) {
		responses.add(FrontendMessage::UpdateMouseCursor { cursor: MouseCursorIcon::Default });
	}
}

impl ToolTransition for TorchTool {
	fn event_to_message_map(&self) -> EventToMessageMap {
		EventToMessageMap {
			tool_abort: Some(TorchToolMessage::Abort.into()),
			overlay_provider: Some(|context| TorchToolMessage::Overlays { context }.into()),
			..Default::default()
		}
	}
}

impl LayoutHolder for TorchTool {
	fn layout(&self) -> Layout {
		Layout::default()
	}
}
