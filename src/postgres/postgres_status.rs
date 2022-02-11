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

use krator::ObjectStatus;
pub use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[doc = "Reflects the status of the current self service postgres database"]
pub struct PostgresStatus {
    pub summary: Option<String>,
    pub deletion_protection: bool,
}

impl Default for PostgresStatus {
    fn default() -> Self {
        PostgresStatus {
            summary: None,
            deletion_protection: true,
        }
    }
}

impl ObjectStatus for PostgresStatus {
    fn json_patch(&self) -> serde_json::Value {
        debug!("json_patch called {:?}", self);
        // Generate a map containing only set fields.

        let mut status = serde_json::Map::new();
        if let Some(summary) = self.summary.clone() {
            debug!("summary: {}", summary);
            status.insert("summary".to_string(), serde_json::Value::String(summary));
        };
        status.insert(
            "deletion_protection".to_string(),
            serde_json::Value::Bool(self.deletion_protection),
        );

        debug!("status: {:?}", status.clone());

        debug!(
            "patch: {}",
            serde_json::json!({ "status": serde_json::Value::Object(status.clone()) })
        );

        // Create status patch with map.
        serde_json::json!({ "status": serde_json::Value::Object(status) })
    }

    fn failed(e: &str) -> PostgresStatus {
        let message = format!("error: {}", e);
        PostgresStatus {
            summary: Some(message),
            deletion_protection: true,
        }
    }
}
