use crate::AsU32;
use crate::choice_type::{ChoiceTypeStatic, ChoiceWidgetHint, VariantMetadata};
use core::fmt::Display;
use num_enum::{FromPrimitive, IntoPrimitive};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, FromPrimitive, IntoPrimitive)]
#[cfg_attr(feature = "std", derive(dyn_any::DynAny, specta::Type, serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum ShadowType {
	#[default]
	Drop = 0,
	Inner = 1,
}

impl Display for ShadowType {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		match self {
			ShadowType::Drop => write!(f, "Drop Shadow"),
			ShadowType::Inner => write!(f, "Inner Shadow"),
		}
	}
}

impl AsU32 for ShadowType {
	fn as_u32(&self) -> u32 {
		*self as u32
	}
}

impl ChoiceTypeStatic for ShadowType {
	const WIDGET_HINT: ChoiceWidgetHint = ChoiceWidgetHint::Dropdown;
	const DESCRIPTION: Option<&'static str> = Some("Select the type of shadow to apply");

	fn list() -> &'static [&'static [(Self, VariantMetadata)]] {
		static ENTRIES: &[(ShadowType, VariantMetadata)] = &[
			(
				ShadowType::Drop,
				VariantMetadata {
					name: "Drop",
					label: "Drop Shadow",
					description: Some("Cast a shadow behind the object"),
					icon: None,
				},
			),
			(
				ShadowType::Inner,
				VariantMetadata {
					name: "Inner",
					label: "Inner Shadow",
					description: Some("Cast a shadow inside the object"),
					icon: None,
				},
			),
		];
		static LIST: &[&[(ShadowType, VariantMetadata)]] = &[ENTRIES];
		LIST
	}
}
