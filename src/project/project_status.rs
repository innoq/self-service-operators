/*
 * Copyright 2021 Daniel Bornkessel
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use handlebars::JsonValue;
use krator::ObjectStatus;

pub use schemars::JsonSchema;

use crate::project::states::ProjectPhase;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[doc = "Reflects the status of the current self service project"]
pub struct ProjectStatus {
    pub phase: Option<ProjectPhase>,
    pub message: Option<String>,
    pub summary: Option<String>,
    pub applied_one_shot_resources: Vec<String>,
}

impl Default for ProjectStatus {
    fn default() -> Self {
        ProjectStatus {
            phase: None,
            message: None,
            summary: None,
            applied_one_shot_resources: vec![],
        }
    }
}

impl ObjectStatus for ProjectStatus {
    fn json_patch(&self) -> serde_json::Value {
        debug!("json_patch called {:?}", self);
        // Generate a map containing only set fields.

        let mut status = serde_json::Map::new();

        if let Some(phase) = self.phase.clone() {
            debug!("phase: {:?}", phase);
            status.insert("phase".to_string(), serde_json::json!(phase));
        };

        if let Some(message) = self.message.clone() {
            debug!("message: {}", message);
            status.insert("message".to_string(), serde_json::Value::String(message));
        };

        if let Some(summary) = self.summary.clone() {
            debug!("summary: {}", summary);
            status.insert("summary".to_string(), serde_json::Value::String(summary));
        };

        status.insert(
            "appliedOneShotResources".to_string(),
            serde_json::Value::Array(
                self.applied_one_shot_resources
                    .clone()
                    .iter()
                    .map(|x| serde_json::to_value(x).unwrap())
                    .collect::<Vec<JsonValue>>(),
            ),
        );

        debug!("status: {:?}", status.clone());

        debug!(
            "patch: {}",
            serde_json::json!({ "status": serde_json::Value::Object(status.clone()) })
        );

        // Create status patch with map.
        serde_json::json!({ "status": serde_json::Value::Object(status) })
    }

    fn failed(e: &str) -> ProjectStatus {
        let message = format!("error: {}", e);
        ProjectStatus {
            summary: Some(crate::project::shorten_string(&message)),
            message: Some(message),
            phase: None,
            applied_one_shot_resources: vec![],
        }
    }
}
