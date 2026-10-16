//! Definition normalizers that keep cosmetic deparse differences from being
//! reported as changes.

pub fn normalize_function_def(def: &str) -> String {
    def.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Normalize constraint definition to reduce false positives from formatting differences.
/// Removes type casts like ::character varying, ::text, etc. and normalizes whitespace.
pub fn normalize_constraint_definition(def: &str) -> String {
    let mut result = def.to_string();

    // Remove type casts
    result = result.replace("::character varying[]", "");
    result = result.replace("::character varying", "");
    result = result.replace("::text[]", "");
    result = result.replace("::text", "");

    // Convert any non-alphanumeric characters to a single space
    let mut normalized = String::with_capacity(result.len());
    let mut last_was_space = true;
    for c in result.chars() {
        if c.is_alphanumeric() {
            normalized.push(c);
            last_was_space = false;
        } else if !last_was_space {
            normalized.push(' ');
            last_was_space = true;
        }
    }

    normalized.trim().to_string()
}

pub fn normalize_trigger_def(def: &str) -> String {
    def.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

/// Normalize a `pg_get_viewdef` result for comparison.
///
/// `pg_get_viewdef` output does not always round-trip. An `IN` list on a
/// `varchar` column deparses as `ARRAY['a'::character varying, ...]::text[]`,
/// but re-creating the view from that text pushes the cast down to each
/// element, so it deparses as `ARRAY['a'::character varying::text, ...]`.
/// Both mean the same thing, so the array-level cast is distributed over the
/// elements to give one canonical form.
pub fn normalize_view_definition(def: &str) -> String {
    distribute_array_casts(def.trim())
}

/// Rewrite `ARRAY[e1, e2]::T[]` to `ARRAY[e1::T, e2::T]` wherever every element
/// is a simple literal or column reference. Anything else is left untouched.
fn distribute_array_casts(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    while let Some(c) = sql[i..].chars().next() {
        let rest = &sql[i..];
        if c == '\'' || c == '"' {
            let len = quoted_len(rest);
            out.push_str(&rest[..len]);
            i += len;
        } else if rest.starts_with("ARRAY[") && !ends_with_ident_char(&sql[..i]) {
            if let Some((rewritten, consumed)) = rewrite_array_cast(rest) {
                out.push_str(&rewritten);
                i += consumed;
            } else {
                // Keep scanning inside so nested arrays still get normalized
                out.push_str("ARRAY[");
                i += "ARRAY[".len();
            }
        } else {
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// Given text starting with `ARRAY[`, return the rewritten array and the
/// number of bytes it replaces, if it is a castable array of simple elements.
fn rewrite_array_cast(s: &str) -> Option<(String, usize)> {
    let body_start = "ARRAY[".len();
    let mut depth = 0usize;
    let mut element_start = body_start;
    let mut elements = Vec::new();
    let mut i = body_start;
    let body_end = loop {
        let c = s[i..].chars().next()?;
        match c {
            '\'' | '"' => {
                i += quoted_len(&s[i..]);
                continue;
            }
            '[' | '(' => depth += 1,
            ']' if depth == 0 => {
                elements.push(s[element_start..i].trim());
                break i;
            }
            ']' | ')' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                elements.push(s[element_start..i].trim());
                element_start = i + 1;
            }
            _ => {}
        }
        i += c.len_utf8();
    };

    let cast = s[body_end + 1..].strip_prefix("::")?;
    let type_len = type_name_len(cast)?;
    let after_type = cast[type_len..].strip_prefix("[]")?;
    if after_type.starts_with('[') || !elements.iter().all(|e| is_simple_element(e)) {
        return None;
    }

    let type_name = &cast[..type_len];
    let rewritten = elements
        .iter()
        .map(|e| format!("{}::{}", e, type_name))
        .collect::<Vec<_>>()
        .join(", ");
    let consumed = s.len() - after_type.len();
    Some((format!("ARRAY[{}]", rewritten), consumed))
}

/// A quoted literal, number, or (qualified) column reference, optionally
/// followed by casts.
fn is_simple_element(e: &str) -> bool {
    let Some(mut rest) = operand_rest(e) else {
        return false;
    };
    while let Some(cast) = rest.strip_prefix("::") {
        let Some(len) = type_name_len(cast) else {
            return false;
        };
        rest = &cast[len..];
        rest = rest.strip_prefix("[]").unwrap_or(rest);
    }
    rest.is_empty()
}

/// Skip a leading literal, number, or identifier chain and return the rest.
fn operand_rest(e: &str) -> Option<&str> {
    let first = e.chars().next()?;
    if first == '\'' {
        return Some(&e[quoted_len(e)..]);
    }
    if first.is_ascii_digit() {
        let len = e
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(e.len());
        return Some(&e[len..]);
    }
    let mut rest = &e[ident_len(e)?..];
    while let Some(next) = rest.strip_prefix('.') {
        rest = &next[ident_len(next)?..];
    }
    Some(rest)
}

/// Length of a type name as `format_type` prints it, e.g. `text`,
/// `character varying(255)`, `timestamp(3) with time zone`, `app."MyType"`.
fn type_name_len(s: &str) -> Option<usize> {
    let mut len = ident_len(s)?;
    loop {
        let rest = &s[len..];
        if rest.starts_with('(') {
            let close = rest.find(')')?;
            if !rest[1..close]
                .chars()
                .all(|c| c.is_ascii_digit() || c == ',' || c == ' ')
            {
                return None;
            }
            len += close + 1;
        } else if let Some(next) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(' ')) {
            match ident_len(next) {
                Some(word) => len += 1 + word,
                None => return Some(len),
            }
        } else {
            return Some(len);
        }
    }
}

/// Length of a leading bare or double-quoted identifier.
fn ident_len(s: &str) -> Option<usize> {
    let first = s.chars().next()?;
    if first == '"' {
        return Some(quoted_len(s));
    }
    if !(first.is_alphabetic() || first == '_') {
        return None;
    }
    Some(s.find(|c: char| !is_ident_char(c)).unwrap_or(s.len()))
}

/// Length of a leading quoted token (`'...'` or `"..."`), with doubled quotes
/// as escapes. An unterminated token runs to the end of the input.
fn quoted_len(s: &str) -> usize {
    let mut chars = s.char_indices();
    let Some((_, quote)) = chars.next() else {
        return 0;
    };
    while let Some((i, c)) = chars.next() {
        if c == quote {
            if s[i + 1..].starts_with(quote) {
                chars.next();
            } else {
                return i + 1;
            }
        }
    }
    s.len()
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn ends_with_ident_char(s: &str) -> bool {
    s.chars().next_back().is_some_and(is_ident_char)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_in_list_matches_its_round_tripped_form() {
        let original = " SELECT file_guid\n   FROM file_struct.file\n  WHERE key_prefix::text = ANY (ARRAY['gnctl_video'::character varying, 'gnctl_audio'::character varying]::text[]);";
        let round_tripped = " SELECT file_guid\n   FROM file_struct.file\n  WHERE key_prefix::text = ANY (ARRAY['gnctl_video'::character varying::text, 'gnctl_audio'::character varying::text]);";
        assert_eq!(
            normalize_view_definition(original),
            normalize_view_definition(round_tripped)
        );
    }

    #[test]
    fn view_with_different_array_values_still_differs() {
        let a = "SELECT 1 WHERE k::text = ANY (ARRAY['a'::character varying, 'b'::character varying]::text[]);";
        let b = "SELECT 1 WHERE k::text = ANY (ARRAY['a'::character varying::text, 'c'::character varying::text]);";
        assert_ne!(normalize_view_definition(a), normalize_view_definition(b));
    }

    #[test]
    fn distributes_multi_word_and_qualified_types() {
        assert_eq!(
            distribute_array_casts("ARRAY['x'::text, t.c]::character varying(10)[]"),
            "ARRAY['x'::text::character varying(10), t.c::character varying(10)]"
        );
        assert_eq!(
            distribute_array_casts("ARRAY[1, 2]::app.\"My Type\"[] AS v"),
            "ARRAY[1::app.\"My Type\", 2::app.\"My Type\"] AS v"
        );
    }

    #[test]
    fn leaves_complex_elements_and_strings_alone() {
        let complex = "ARRAY[lower(a), b]::text[]";
        assert_eq!(distribute_array_casts(complex), complex);

        let expression = "ARRAY[a + 1, b]::numeric[]";
        assert_eq!(distribute_array_casts(expression), expression);

        let in_string = "SELECT 'ARRAY[''a'']::text[]'::text";
        assert_eq!(distribute_array_casts(in_string), in_string);

        let uncast = "ARRAY['a'::text, 'b'::text]";
        assert_eq!(distribute_array_casts(uncast), uncast);
    }

    #[test]
    fn normalizes_nested_arrays_from_the_inside() {
        assert_eq!(
            distribute_array_casts("ARRAY[ARRAY['a'::character varying]::text[]]"),
            "ARRAY[ARRAY['a'::character varying::text]]"
        );
    }
}
