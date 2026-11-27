//! Serializable workbench data shared by Web renderers. UI packages and assets live in adapters.

use serde::{Deserialize, Serialize};

use crate::{ArgSchema, RegisteredCommand, ViewSlot};

#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiCommand {
    pub name: String,
    pub about: String,
    pub args: Vec<ArgSchema>,
    pub views: Vec<ViewSlot>,
}

impl From<&RegisteredCommand> for UiCommand {
    fn from(command: &RegisteredCommand) -> Self {
        Self {
            name: command.name.clone(),
            about: command.schema.about.clone(),
            args: command.schema.args.clone(),
            views: command.views.clone(),
        }
    }
}

#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiCatalog {
    pub commands: Vec<UiCommand>,
    pub selected: String,
    pub can_cancel: bool,
    pub protocol_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<UiCli>,
}

/// Local executable and calling convention for the copyable command.
#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiCli {
    pub program: String,
    pub uses_subcommand: bool,
}

#[cfg_attr(feature = "ui-schema", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiRunRequest {
    pub args: std::collections::HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub cmd: Option<String>,
}

/// Schema export roots. The Web renderer receives each field through its own HTTP operation.
#[cfg(feature = "ui-schema")]
#[derive(schemars::JsonSchema)]
pub struct UiContract {
    pub catalog: UiCatalog,
    pub event: crate::Progress,
    pub request: UiRunRequest,
}
