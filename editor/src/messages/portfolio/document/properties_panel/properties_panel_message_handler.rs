use graphene_std::uuid::NodeId;

use crate::messages::layout::utility_types::widget_prelude::*;
use crate::messages::portfolio::document::node_graph::document_node_definitions::NodePropertiesContext;
use crate::messages::portfolio::document::utility_types::network_interface::NodeNetworkInterface;
use crate::messages::portfolio::utility_types::PersistentData;
use crate::messages::prelude::*;
use crate::node_graph_executor::NodeGraphExecutor;

#[derive(ExtractField)]
pub struct PropertiesPanelMessageContext<'a> {
	pub network_interface: &'a mut NodeNetworkInterface,
	pub selection_network_path: &'a [NodeId],
	pub document_name: &'a str,
	pub executor: &'a mut NodeGraphExecutor,
	pub persistent_data: &'a PersistentData,
	pub properties_panel_open: bool,
}

#[derive(Debug, Clone, Default, ExtractField)]
pub struct PropertiesPanelMessageHandler {}

#[message_handler_data]
impl MessageHandler<PropertiesPanelMessage, PropertiesPanelMessageContext<'_>> for PropertiesPanelMessageHandler {
	fn process_message(&mut self, message: PropertiesPanelMessage, responses: &mut VecDeque<Message>, context: PropertiesPanelMessageContext) {
		let PropertiesPanelMessageContext {
			network_interface,
			selection_network_path,
			document_name,
			executor,
			persistent_data,
			properties_panel_open,
		} = context;

		match message {
			PropertiesPanelMessage::Clear => {
				responses.add(LayoutMessage::SendLayout {
					layout: Layout::default(),
					layout_target: LayoutTarget::PropertiesPanel,
				});
			}
			PropertiesPanelMessage::Refresh => {
				if !properties_panel_open {
					responses.add(PropertiesPanelMessage::Clear);
					return;
				}

				let mut node_properties_context = NodePropertiesContext {
					persistent_data,
					responses,
					network_interface,
					selection_network_path,
					document_name,
					executor,
				};
				let layout = Layout(NodeGraphMessageHandler::collate_properties(&mut node_properties_context));

				node_properties_context.responses.add(LayoutMessage::SendLayout {
					layout,
					layout_target: LayoutTarget::PropertiesPanel,
				});
			}
			PropertiesPanelMessage::SetSectionCollapsed { node_id, collapsed } => {
				network_interface.set_collapsed(&node_id, selection_network_path, collapsed);
				responses.add(PropertiesPanelMessage::Refresh);
			}
			PropertiesPanelMessage::SetAllSectionsCollapsed { collapsed } => {
				if properties_panel_open {
					set_all_sections_collapsed(
						collapsed,
						NodePropertiesContext {
							persistent_data,
							responses,
							network_interface,
							selection_network_path,
							document_name,
							executor,
						},
					);
				}
			}
		}
	}

	fn actions(&self) -> ActionList {
		actions!(PropertiesMessageDiscriminant;)
	}
}

fn set_all_sections_collapsed(collapsed: bool, mut node_properties_context: NodePropertiesContext) {
	let mut layout = NodeGraphMessageHandler::collate_properties(&mut node_properties_context);

	fn set_collapsed_in_layout(layout: &mut [LayoutGroup], ids: &mut Vec<NodeId>, collapsed: bool) {
		for group in layout {
			if let LayoutGroup::Section {
				id,
				layout,
				collapsed: section_collapsed,
				..
			} = group
			{
				*section_collapsed = collapsed;
				ids.push(NodeId(*id));
				set_collapsed_in_layout(&mut layout.0, ids, collapsed);
			}
		}
	}

	let mut ids = Vec::new();
	set_collapsed_in_layout(&mut layout, &mut ids, collapsed);

	let NodePropertiesContext {
		network_interface,
		selection_network_path,
		responses,
		..
	} = node_properties_context;

	for node_id in ids {
		network_interface.set_collapsed(&node_id, selection_network_path, collapsed);
	}

	responses.add(LayoutMessage::SendLayout {
		layout: Layout(layout),
		layout_target: LayoutTarget::PropertiesPanel,
	});
}
