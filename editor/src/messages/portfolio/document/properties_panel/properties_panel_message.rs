use crate::messages::prelude::*;
use graph_craft::document::NodeId;

#[impl_message(Message, DocumentMessage, PropertiesPanel)]
#[derive(PartialEq, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum PropertiesPanelMessage {
	// Messages
	Clear,
	Refresh,
	SetSectionCollapsed { node_id: NodeId, collapsed: bool },
	SetAllSectionsCollapsed { collapsed: bool },
}
