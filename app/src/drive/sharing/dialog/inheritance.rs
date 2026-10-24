//! Support for displaying inherited ACLs.

use torch_core::ui::appearance::Appearance;
use torchui::ui_components::components::UiComponent as _;
use torchui::{AppContext, Element, SingletonEntity as _};

use super::style;
use crate::cloud_object::ServerObjectContainer;
use crate::cloud_object::model::persistence::CloudModel;
use crate::server::ids::SyncId;

/// UI state for inherited permissions.
pub struct InheritanceState {
    // The server API allows inheriting ACLs from drives as well, but we currently don't use this.
    source_folder: SyncId,
}

impl InheritanceState {
    /// Construct inheritance state for an object and the source of its possibly-inherited ACL.
    pub fn from_object_and_source(
        object_id: &SyncId,
        source: Option<&ServerObjectContainer>,
    ) -> Option<InheritanceState> {
        let source_folder = match source? {
            ServerObjectContainer::Folder { folder_uid } => SyncId::ServerId(*folder_uid),
            _ => return None,
        };
        // ACLs _on_ folders may include themselves as sources.
        if &source_folder == object_id {
            return None;
        }

        Some(InheritanceState { source_folder })
    }

    pub fn details(&self, appearance: &Appearance, app: &AppContext) -> InheritanceDetails {
        let folder_name = CloudModel::as_ref(app)
            .get_folder(&self.source_folder)
            .map(|folder| &folder.model().name);

        let label = match folder_name {
            Some(folder_name) => format!("Inherited from {folder_name}"),
            None => "Inherited permission".to_owned(),
        };
        InheritanceDetails {
            source_label: style::detail_text(label, appearance).build().finish(),
            tooltip_text: "Cannot edit inherited permissions",
        }
    }
}

/// Information to display about inherited permissions.
pub struct InheritanceDetails {
    /// A label element describing where an ACL was inherited from.
    pub source_label: Box<dyn Element>,
    /// A tooltip to show on disabled permission-editing controls.
    pub tooltip_text: &'static str,
}
