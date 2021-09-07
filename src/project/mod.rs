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
