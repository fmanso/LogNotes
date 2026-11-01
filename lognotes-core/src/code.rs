//! Reading a code fence: which language it names, and what in it is a keyword, a string, a comment or a
//! number, so the rendered block can colour it.
//!
//! This is a *lexer*, not a parser, and the difference is deliberate: it never fails, it never rejects an
//! input, and where a language has a construct a lexer cannot follow — a template literal with nested
//! braces, a heredoc — it colours what it can and leaves the rest alone. One scanner serves every family;
//! a family is little more than its comment markers, its quote characters and its keyword list.

/// The families the colouring knows. A fence naming anything else is rendered as plain code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Language {
    Shell,
    PowerShell,
    Python,
    /// C, C++, C#, Java, Rust, Go, JavaScript, TypeScript, JSON, CSS — braces, `//` and quotes.
    CLike,
    /// HTML, XML, SVG, Vue.
    Markup,
    Sql,
}

/// What a token is. Nothing finer: this colours code, it does not describe its grammar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    Keyword,
    String,
    Comment,
    Number,
}

/// A coloured run, as byte offsets into the code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

/// The language a fence names, or `None` for one nobody colours.
pub fn language_of(fence: &str) -> Option<Language> {
    let name = fence.trim().to_ascii_lowercase();
    Some(match name.as_str() {
        "sh" | "bash" | "zsh" | "fish" | "console" | "shell" => Language::Shell,
        "powershell" | "ps1" | "pwsh" => Language::PowerShell,
        "python" | "py" | "python3" => Language::Python,
        "c" | "h" | "cpp" | "c++" | "cxx" | "hpp" | "cc" | "cs" | "csharp" | "java" | "rust"
        | "rs" | "go" | "js" | "javascript" | "jsx" | "ts" | "typescript" | "tsx" | "json"
        | "css" | "scss" | "less" | "kotlin" | "swift" | "objc" => Language::CLike,
        "html" | "xml" | "svg" | "vue" | "xhtml" => Language::Markup,
        "sql" | "sqlite" => Language::Sql,
        _ => return None,
    })
}

/// The tokens of `code`, in order, never overlapping.
pub fn tokens(code: &str, language: Language) -> Vec<Token> {
    let rules = rules(language);
    let mut out = Vec::new();
    let mut index = 0;

    while index < code.len() {
        let rest = &code[index..];
        let character = rest.chars().next().expect("index is on a character");

        if let Some((open, close)) = rules.block_comment
            && rest.starts_with(open)
        {
            let end = code[index + open.len()..]
                .find(close)
                .map(|at| index + open.len() + at + close.len())
                .unwrap_or(code.len());
            push(&mut out, index, end, TokenKind::Comment);
            index = end;
            continue;
        }

        if let Some(marker) = rules
            .line_comments
            .iter()
            .find(|marker| rest.starts_with(**marker))
        {
            let end = code[index..]
                .find('\n')
                .map(|at| index + at)
                .unwrap_or(code.len());
            push(&mut out, index, end, TokenKind::Comment);
            index = end;
            let _ = marker;
            continue;
        }

        if language == Language::Python
            && let Some(end) = triple_quoted(code, index)
        {
            push(&mut out, index, end, TokenKind::String);
            index = end;
            continue;
        }

        if language == Language::Markup && character == '<' {
            // A tag, up to the end of its name; attributes then read as ordinary text and strings.
            let end = rest
                .char_indices()
                .skip(1)
                .find(|(_, c)| c.is_whitespace() || *c == '>' || *c == '/')
                .map(|(at, _)| index + at)
                .unwrap_or(code.len());
            push(&mut out, index, end, TokenKind::Keyword);
            index = end;
            continue;
        }

        if rules.quotes.contains(&character) {
            let end = quoted(code, index, character, rules.escape);
            push(&mut out, index, end, TokenKind::String);
            index = end;
            continue;
        }

        if character.is_ascii_digit() {
            let end = number(code, index);
            push(&mut out, index, end, TokenKind::Number);
            index = end;
            continue;
        }

        if character.is_alphanumeric() || character == '_' {
            let end = word(code, index);
            let text = &code[index..end];
            // SQL and PowerShell read their keywords in any case; the rest do not care either way here.
            let lowered = text.to_ascii_lowercase();
            if rules.keywords.contains(&lowered.as_str()) {
                push(&mut out, index, end, TokenKind::Keyword);
            }
            index = end;
            continue;
        }

        index += character.len_utf8();
    }

    out
}

struct Rules {
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    quotes: &'static [char],
    escape: char,
    keywords: &'static [&'static str],
}

fn rules(language: Language) -> Rules {
    match language {
        Language::Shell => Rules {
            line_comments: &["#"],
            block_comment: None,
            quotes: &['"', '\''],
            escape: '\\',
            keywords: SHELL_KEYWORDS,
        },
        Language::PowerShell => Rules {
            line_comments: &["#"],
            block_comment: Some(("<#", "#>")),
            quotes: &['"', '\''],
            escape: '`',
            keywords: POWERSHELL_KEYWORDS,
        },
        Language::Python => Rules {
            line_comments: &["#"],
            block_comment: None,
            quotes: &['"', '\''],
            escape: '\\',
            keywords: PYTHON_KEYWORDS,
        },
        Language::CLike => Rules {
            line_comments: &["//"],
            block_comment: Some(("/*", "*/")),
            quotes: &['"', '\''],
            escape: '\\',
            keywords: C_LIKE_KEYWORDS,
        },
        Language::Markup => Rules {
            line_comments: &[],
            block_comment: Some(("<!--", "-->")),
            quotes: &['"', '\''],
            escape: '\\',
            keywords: MARKUP_KEYWORDS,
        },
        Language::Sql => Rules {
            line_comments: &["--"],
            block_comment: Some(("/*", "*/")),
            quotes: &['\'', '"'],
            escape: '\'',
            keywords: SQL_KEYWORDS,
        },
    }
}

fn push(out: &mut Vec<Token>, start: usize, end: usize, kind: TokenKind) {
    if end > start {
        out.push(Token { start, end, kind });
    }
}

/// A run of word characters, which is what a keyword can be.
fn word(code: &str, from: usize) -> usize {
    let mut end = from;
    while end < code.len() {
        let character = code[end..].chars().next().expect("on a character");
        if !character.is_alphanumeric() && character != '_' {
            break;
        }
        end += character.len_utf8();
    }
    end
}

/// A number, stopping short of the letters that usually follow it in prose.
fn number(code: &str, from: usize) -> usize {
    let mut end = from;
    let mut seen_dot = false;
    while end < code.len() {
        let character = code[end..].chars().next().expect("on a character");
        let is_hex = code[from..end].eq_ignore_ascii_case("0x") && character.is_ascii_hexdigit();
        if character.is_ascii_digit() || is_hex || character == '_' {
            end += 1;
            continue;
        }
        if character == '.' && !seen_dot && end + 1 < code.len() {
            let next = code[end + 1..].chars().next().expect("on a character");
            if next.is_ascii_digit() {
                seen_dot = true;
                end += 1;
                continue;
            }
        }
        break;
    }
    end
}

/// A quoted string, ending at its closing quote, at the end of its line if the quote never comes, or at
/// the end of the code — whichever is first. A stray quote must not paint the rest of a fence.
fn quoted(code: &str, from: usize, quote: char, escape: char) -> usize {
    let mut index = from + quote.len_utf8();
    while index < code.len() {
        let character = code[index..].chars().next().expect("on a character");
        if character == escape {
            index += character.len_utf8();
            if index < code.len() {
                index += code[index..].chars().next().expect("on a character").len_utf8();
            }
            continue;
        }
        index += character.len_utf8();
        if character == quote {
            return index;
        }
        if character == '\n' {
            return index - 1;
        }
    }
    code.len()
}

/// Python's `"""…"""` and `'''…'''`, which may span lines.
fn triple_quoted(code: &str, from: usize) -> Option<usize> {
    let rest = &code[from..];
    let marker = ["\"\"\"", "'''"]
        .into_iter()
        .find(|marker| rest.starts_with(marker))?;
    let after = from + marker.len();
    let end = code[after..]
        .find(marker)
        .map(|at| after + at + marker.len())
        .unwrap_or(code.len());
    Some(end)
}

const SHELL_KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "function", "in", "return", "local", "export", "alias", "unset", "source", "echo", "cd", "pwd",
    "exit", "set", "shift", "trap", "read", "test", "true", "false", "break", "continue",
];

const POWERSHELL_KEYWORDS: &[&str] = &[
    "if", "elseif", "else", "switch", "foreach", "for", "while", "do", "until", "function", "param",
    "return", "begin", "process", "end", "try", "catch", "finally", "throw", "filter", "in", "not",
    "and", "or", "eq", "ne", "gt", "lt", "write-host", "write-output", "get-childitem", "select-object",
];

const PYTHON_KEYWORDS: &[&str] = &[
    "def", "class", "return", "if", "elif", "else", "for", "while", "in", "import", "from", "as",
    "with", "try", "except", "finally", "raise", "yield", "lambda", "pass", "break", "continue", "and",
    "or", "not", "is", "none", "true", "false", "global", "nonlocal", "assert", "del", "async", "await",
];

const C_LIKE_KEYWORDS: &[&str] = &[
    "if", "else", "for", "while", "do", "switch", "case", "default", "break", "continue", "return",
    "goto", "sizeof", "typedef", "struct", "union", "enum", "static", "extern", "const", "volatile",
    "inline", "register", "signed", "unsigned", "short", "long", "int", "char", "float", "double",
    "void", "bool", "true", "false", "null", "nullptr", "this", "new", "delete", "class", "public",
    "private", "protected", "virtual", "override", "final", "namespace", "using", "template",
    "typename", "try", "catch", "finally", "throw", "throws", "interface", "extends", "implements",
    "package", "import", "export", "default", "async", "await", "yield", "function", "typeof",
    "instanceof", "in", "of", "let", "var", "fn", "pub", "impl", "trait", "mod", "use", "match", "loop",
    "mut", "ref", "dyn", "where", "self", "super", "crate", "go", "defer", "chan", "select", "range",
    "type", "map", "func", "string", "number", "boolean", "any", "unknown", "never", "undefined",
];

const MARKUP_KEYWORDS: &[&str] = &[];

const SQL_KEYWORDS: &[&str] = &[
    "select", "from", "where", "insert", "into", "values", "update", "set", "delete", "join", "left",
    "right", "inner", "outer", "full", "cross", "on", "group", "by", "order", "having", "limit",
    "offset", "create", "table", "view", "index", "trigger", "drop", "alter", "add", "column", "as",
    "and", "or", "not", "null", "is", "in", "like", "between", "exists", "distinct", "count", "sum",
    "avg", "min", "max", "case", "when", "then", "else", "end", "union", "all", "primary", "key",
    "foreign", "references", "unique", "default", "asc", "desc", "pragma",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn coloured(code: &str, language: Language) -> Vec<(String, TokenKind)> {
        tokens(code, language)
            .into_iter()
            .map(|token| (code[token.start..token.end].to_string(), token.kind))
            .collect()
    }

    #[test]
    fn fences_name_their_family() {
        assert_eq!(language_of("bash"), Some(Language::Shell));
        assert_eq!(language_of("zsh"), Some(Language::Shell));
        assert_eq!(language_of("Python"), Some(Language::Python));
        assert_eq!(language_of("csharp"), Some(Language::CLike));
        assert_eq!(language_of("pwsh"), Some(Language::PowerShell));
        assert_eq!(language_of("html"), Some(Language::Markup));
        assert_eq!(language_of("sql"), Some(Language::Sql));
        assert_eq!(language_of("mermaid"), None);
        assert_eq!(language_of(""), None);
    }

    #[test]
    fn a_shell_fence_colours_comments_strings_keywords_and_numbers() {
        let found = coloured("for f in \"$x\"; do # go\n  echo 2\n done", Language::Shell);

        assert!(found.contains(&("for".into(), TokenKind::Keyword)));
        assert!(found.contains(&("do".into(), TokenKind::Keyword)));
        assert!(found.contains(&("\"$x\"".into(), TokenKind::String)));
        assert!(found.contains(&("# go".into(), TokenKind::Comment)));
        assert!(found.contains(&("2".into(), TokenKind::Number)));
    }

    #[test]
    fn a_config_file_uses_the_shell_family_for_its_hash_comments() {
        let found = coloured("key = value # a note", Language::Shell);

        assert_eq!(found, [("# a note".to_string(), TokenKind::Comment)]);
    }

    #[test]
    fn a_python_fence_colours_a_triple_quoted_string_across_lines() {
        let code = "def f():\n    \"\"\"One\n    two\"\"\"\n    return 1";
        let found = coloured(code, Language::Python);

        assert!(found.contains(&("def".into(), TokenKind::Keyword)));
        assert!(found.iter().any(|(text, kind)| *kind == TokenKind::String
            && text.starts_with("\"\"\"One") && text.ends_with("\"\"\"")));
        assert!(found.contains(&("1".into(), TokenKind::Number)));
    }

    #[test]
    fn a_c_fence_colours_a_block_comment_and_a_line_comment() {
        let code = "/* why\n   not */\nint x = 1; // trailing";
        let found = coloured(code, Language::CLike);

        assert!(found.iter().any(|(text, kind)| *kind == TokenKind::Comment
            && text.starts_with("/* why") && text.ends_with("*/")));
        assert!(found.contains(&("int".into(), TokenKind::Keyword)));
        assert!(found.contains(&("// trailing".into(), TokenKind::Comment)));
        assert!(found.contains(&("1".into(), TokenKind::Number)));
    }

    #[test]
    fn sql_keywords_are_case_insensitive() {
        let found = coloured("SELECT id FROM t where x = 1", Language::Sql);

        assert!(found.contains(&("SELECT".into(), TokenKind::Keyword)));
        assert!(found.contains(&("FROM".into(), TokenKind::Keyword)));
        assert!(found.contains(&("where".into(), TokenKind::Keyword)));
    }

    #[test]
    fn a_markup_fence_colours_tags_and_comments() {
        let found = coloured("<div class=\"x\"><!-- note --></div>", Language::Markup);

        assert!(found.contains(&("<div".into(), TokenKind::Keyword)));
        assert!(found.contains(&("\"x\"".into(), TokenKind::String)));
        assert!(found.contains(&("<!-- note -->".into(), TokenKind::Comment)));
    }

    #[test]
    fn a_stray_quote_stops_at_the_end_of_its_line() {
        let code = "echo \"unclosed\nstill plain text here";

        let found = coloured(code, Language::Shell);

        assert_eq!(found.iter().filter(|(_, kind)| *kind == TokenKind::String).count(), 1);
        assert!(found.contains(&("\"unclosed".into(), TokenKind::String)));
    }

    #[test]
    fn nothing_coloured_leaves_plain_prose_alone() {
        assert!(tokens("just some words, no code", Language::CLike).is_empty());
    }

    #[test]
    fn tokens_never_overlap_and_stay_inside_the_code() {
        let code = "if (x < 3) { /* c */ return \"s\"; }";

        let found = tokens(code, Language::CLike);
        let mut last = 0;
        for token in &found {
            assert!(token.start >= last, "overlapping tokens in {code:?}");
            assert!(token.end <= code.len());
            assert!(code.is_char_boundary(token.start) && code.is_char_boundary(token.end));
            last = token.end;
        }
    }

    #[test]
    fn a_number_stops_at_the_next_word() {
        let found = coloured("2nd place", Language::CLike);

        assert_eq!(found, [("2".to_string(), TokenKind::Number)]);
    }
}
