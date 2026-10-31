//! Reading links out of block text.
//!
//! Block text is the only truth about what a block links to: the extracted rows are derived state that
//! can be thrown away and rebuilt, which is why extraction runs on every write rather than being
//! authored by a separate command.

/// What a link names and how it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtractedLink {
    pub target: String,
    pub kind: &'static str,
}

/// `[[Page Name]]` and `#tag`, in the order they appear, deduplicated by target.
///
/// Spans inside inline code are skipped: a code sample that happens to contain `[[x]]` must not create a
/// page. Aliases (`[label]([[target]])`) and `#[[multi word tags]]` are not supported yet.
pub(crate) fn extract_links(content: &str) -> Vec<ExtractedLink> {
    let mut links: Vec<ExtractedLink> = Vec::new();
    let bytes: Vec<char> = content.chars().collect();
    let mut index = 0;
    let mut in_code = false;

    while index < bytes.len() {
        let character = bytes[index];

        if character == '`' {
            in_code = !in_code;
            index += 1;
            continue;
        }
        if in_code {
            index += 1;
            continue;
        }

        if character == '[' && index + 1 < bytes.len() && bytes[index + 1] == '[' {
            if let Some(end) = find(&bytes, index + 2, "]]") {
                let target: String = bytes[index + 2..end].iter().collect();
                push(&mut links, target.trim(), "link");
                index = end + 2;
                continue;
            }
        }

        if character == '#' {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && !bytes[end].is_whitespace() && !is_tag_break(bytes[end]) {
                end += 1;
            }
            if end > start {
                let target: String = bytes[start..end].iter().collect();
                push(&mut links, target.trim_end_matches(&['.', ',', ';', ':'][..]), "tag");
                index = end;
                continue;
            }
        }

        index += 1;
    }

    links
}

fn is_tag_break(character: char) -> bool {
    matches!(character, '#' | '[' | ']' | '(' | ')' | '`')
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

fn find(haystack: &[char], from: usize, needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    if from >= haystack.len() {
        return None;
    }
    (from..haystack.len().saturating_sub(needle.len() - 1)).find(|start| {
        haystack[*start..*start + needle.len()] == needle[..]
    })
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
