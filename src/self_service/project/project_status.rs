use krator::ObjectStatus;

pub use schemars::JsonSchema;

use crate::self_service;
use crate::self_service::project::states::ProjectPhase;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[doc = "Reflects the status of the current self service project"]
pub struct ProjectStatus {
    pub(crate) phase: Option<ProjectPhase>,
    pub message: Option<String>,
    pub summary: Option<String>,
}

impl ObjectStatus for ProjectStatus {
    fn json_patch(&self) -> serde_json::Value {
        debug!("json_patch called {:?}", self);
        // Generate a map containing only set fields.

        let mut status = serde_json::Map::new();

        if let Some(phase) = self.phase.clone() {
            status.insert("phase".to_string(), serde_json::json!(phase));
        };

        if let Some(message) = self.message.clone() {
            status.insert("message".to_string(), serde_json::Value::String(message));
        };

        if let Some(summary) = self.summary.clone() {
            status.insert("summary".to_string(), serde_json::Value::String(summary));
        };

        // Create status patch with map.
        serde_json::json!({ "status": serde_json::Value::Object(status) })
    }

    fn failed(e: &str) -> ProjectStatus {
        let message = format!("error: {}", e);
        ProjectStatus {
            summary: Some(self_service::project::shorten_string(&message)),
            message: Some(message),
            phase: None,
        }
    }
}
