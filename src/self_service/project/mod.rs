pub mod operator;
pub mod project;
pub mod states;

pub use project::{Project, ProjectSpec, ProjectStatus, Sample};

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
