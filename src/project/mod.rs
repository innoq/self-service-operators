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

pub use project::{Project, ProjectSpec, Sample};
pub use project_status::ProjectStatus;

pub mod operator;
pub mod project;
mod project_status;
pub mod states;

pub fn shorten_string(s: &str) -> String {
    let max_length = 50;
    let mut s = s.to_string();
    if s.len() > max_length {
        s = s.replace("\n", " ");
        s.truncate(max_length - 3);
        s.push_str("...");
    }

    s
}
