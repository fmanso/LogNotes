//! Drawing a block's ```mermaid fence.
//!
//! Rendering happens on the worker thread, never on the GUI thread: the view is handed a `file://` URL
//! it can load, and the file is named after its content, so a snapshot — which happens after every edit
//! anywhere in the column — never redraws a diagram that has not changed.

use std::collections::HashMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use lognotes_core::markdown;

/// What the column shows for a block that carries a diagram.
#[derive(Clone, Debug, Default)]
pub struct Drawn {
    /// A `file://` URL for the rendered SVG, empty when the code did not parse.
    pub source: String,
    /// One line for the view to show under the fence, empty when it rendered.
    pub error: String,
}

/// Diagrams already drawn, by the hash of the code that produced them.
#[derive(Default)]
pub struct Diagrams {
    drawn: HashMap<u64, Result<String, String>>,
}

impl Diagrams {
    /// The diagram for a block's text, or `None` when it has no mermaid fence.
    pub fn for_text(&mut self, text: &str) -> Option<Drawn> {
        let fence = markdown::mermaid_fence(text)?;
        let key = hash(&fence.code);
        let drawn = self
            .drawn
            .entry(key)
            .or_insert_with(|| render(&fence.code, key))
            .clone();

        Some(match drawn {
            Ok(source) => Drawn {
                source,
                error: String::new(),
            },
            Err(error) => Drawn {
                source: String::new(),
                error,
            },
        })
    }
}

fn render(code: &str, key: u64) -> Result<String, String> {
    let svg = mermaid_rs_renderer::render(code).map_err(|error| format!("diagram error: {}", first_line(&error.to_string())))?;

    let path = cache_dir().join(format!("{key:016x}.svg"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&path, svg).map_err(|error| error.to_string())?;

    Ok(file_url(&path))
}

/// The cache lives in the OS temp dir rather than an application directory: it is derived data, so it
/// can be thrown away at any time, and keeping it there leaves the database path the only place this
/// codebase has to know an OS-specific location.
fn cache_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("lognotes-diagrams")
}

fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/").replace(' ', "%20");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        // A Windows drive path needs the third slash: file:///C:/…
        format!("file:///{text}")
    }
}

/// A diagram error is a parse message, often several lines; the view has room for one.
fn first_line(message: &str) -> String {
    message
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("mermaid could not read this diagram")
        .to_string()
}

fn hash(code: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    code.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flowchart_is_drawn_once_and_reused() {
        let mut diagrams = Diagrams::default();
        let text = "```mermaid\ngraph TD\n  A --> B\n```";

        let first = diagrams.for_text(text).expect("a mermaid fence");
        assert!(first.error.is_empty(), "expected a diagram, got {}", first.error);
        assert!(first.source.starts_with("file://"), "got {}", first.source);

        let again = diagrams.for_text(text).expect("a mermaid fence");
        assert_eq!(again.source, first.source, "the same code draws the same file");
    }

    #[test]
    fn a_broken_diagram_reports_one_quiet_line() {
        let mut diagrams = Diagrams::default();
        let drawn = diagrams
            .for_text("```mermaid\ngraph TD\n  A -->\n```")
            .expect("a mermaid fence");

        assert!(drawn.source.is_empty());
        assert!(drawn.error.starts_with("diagram error: "), "got {}", drawn.error);
        assert!(!drawn.error.contains('\n'));
    }

    #[test]
    fn a_fence_with_no_body_yet_is_not_a_crash() {
        // The moment ```mermaid is typed the fence is a fence with nothing in it — the debounce will
        // render that, long before a person has typed a single line of diagram.
        let mut diagrams = Diagrams::default();
        let drawn = diagrams.for_text("```mermaid").expect("a mermaid fence");

        assert!(drawn.source.is_empty());
        assert!(drawn.error.starts_with("diagram error: "), "got {:?}", drawn.error);
    }

    #[test]
    fn ordinary_code_is_not_a_diagram() {
        let mut diagrams = Diagrams::default();

        assert!(diagrams.for_text("```rust\nfn main() {}\n```").is_none());
        assert!(diagrams.for_text("just a note").is_none());
    }

    #[test]
    fn the_url_is_one_qt_can_load() {
        assert_eq!(file_url(Path::new("/tmp/a b.svg")), "file:///tmp/a%20b.svg");
        assert_eq!(file_url(Path::new("C:\\Temp\\a.svg")), "file:///C:/Temp/a.svg");
    }
}
