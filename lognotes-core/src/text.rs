//! Reading links out of block text.
//!
//! Block text is the only truth about what a block links to: the extracted rows are derived state that
//! can be thrown away and rebuilt, which is why extraction runs on every write rather than being
//! authored by a separate command.

use crate::markdown::LinkKind;

/// What a link names and how it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtractedLink {
    pub target: String,
    pub kind: &'static str,
}

/// `[[Page Name]]` and `#tag`, in the order they appear, deduplicated by target.
///
/// The scanning itself lives in [`crate::markdown`], which the editor's styling and the rendered
/// summaries also read: one scanner means a code sample that contains `[[x]]` is neither a link nor
/// styled like one, and the two can never drift apart. Markdown links (`[label](url)`) point outside the
/// graph, so they are not page references.
pub(crate) fn extract_links(content: &str) -> Vec<ExtractedLink> {
    let mut links: Vec<ExtractedLink> = Vec::new();
    for link in crate::markdown::scan(content).links {
        let kind = match link.kind {
            LinkKind::Page => "link",
            LinkKind::Tag => "tag",
            LinkKind::Url => continue,
        };
        push(&mut links, &link.target, kind);
    }
    links
}

/// Tags follow page-name rules: no spaces, no leading punctuation.
fn push(links: &mut Vec<ExtractedLink>, target: &str, kind: &'static str) {
    if target.is_empty() || target.starts_with('#') {
        return;
    }
    if links.iter().any(|link| link.target.eq_ignore_ascii_case(target)) {
        return;
    }
    links.push(ExtractedLink {
        target: target.to_string(),
        kind,
    });
}

/// True when `needle` appears in `text` as whole words, case-insensitively.
pub(crate) fn mentions_whole_words(text: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let haystack = text.to_lowercase();
    let needle = needle.to_lowercase();
    let mut from = 0;

    while let Some(at) = haystack[from..].find(&needle) {
        let start = from + at;
        let end = start + needle.len();
        let before_ok = haystack[..start]
            .chars()
            .next_back()
            .map(|c| !c.is_alphanumeric())
            .unwrap_or(true);
        let after_ok = haystack[end..]
            .chars()
            .next()
            .map(|c| !c.is_alphanumeric())
            .unwrap_or(true);
        if before_ok && after_ok {
            return true;
        }
        from = end;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(content: &str) -> Vec<String> {
        extract_links(content)
            .into_iter()
            .map(|link| link.target)
            .collect()
    }

    #[test]
    fn reads_links_and_tags_in_order() {
        let found = extract_links("see [[My Project]] and #idea then [[Another Page]]");

        assert_eq!(found.len(), 3);
        assert_eq!(found[0].target, "My Project");
        assert_eq!(found[0].kind, "link");
        assert_eq!(found[1].target, "idea");
        assert_eq!(found[1].kind, "tag");
        assert_eq!(found[2].target, "Another Page");
    }

    #[test]
    fn keeps_one_row_per_target() {
        assert_eq!(targets("[[A]] [[a]] #A"), ["A"], "case-insensitive dedupe");
    }

    #[test]
    fn skips_code_spans() {
        assert!(targets("a `[[Not A Link]]` b").is_empty());
        assert_eq!(targets("`x` [[Real]]"), ["Real"]);
    }

    #[test]
    fn stops_a_tag_at_punctuation() {
        assert_eq!(targets("#idea, and #two."), ["idea", "two"]);
    }

    #[test]
    fn ignores_unclosed_and_empty_brackets() {
        assert!(targets("[[unclosed").is_empty());
        assert!(targets("[[]]").is_empty());
    }

    #[test]
    fn mentions_match_whole_words_only() {
        assert!(mentions_whole_words("talk about Pla today", "Pla"));
        assert!(mentions_whole_words("PLA is here", "pla"));
        assert!(!mentions_whole_words("Plastic and Plaque", "Pla"));
        assert!(mentions_whole_words("Plastic, and Pla.", "Pla"));
        assert!(mentions_whole_words("see [[My Project]] now", "My Project"));
    }
}
