use std::collections::BTreeSet;

pub fn is_stopword(token: &str) -> bool {
    matches!(
        token,
        "the"
            | "and"
            | "for"
            | "from"
            | "with"
            | "that"
            | "this"
            | "what"
            | "when"
            | "where"
            | "which"
            | "who"
            | "how"
            | "did"
            | "does"
            | "was"
            | "were"
            | "are"
            | "has"
            | "had"
            | "have"
            | "about"
            | "into"
            | "onto"
            | "their"
            | "there"
            | "they"
            | "them"
            | "your"
            | "you"
            | "our"
            | "ours"
            | "his"
            | "her"
            | "she"
            | "him"
            | "can"
            | "could"
            | "would"
            | "should"
            | "will"
            | "shall"
    )
}

pub fn is_meaningful_token(token: &str) -> bool {
    token.len() > 2 && !is_stopword(token)
}

pub fn clamp_u8(base: u8, delta: i8) -> u8 {
    let next = base as i16 + delta as i16;
    next.clamp(0, 100) as u8
}

pub fn json_escape(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len() + 8);
    for ch in input.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

pub fn quote(input: &str) -> String {
    format!("\"{}\"", json_escape(input))
}

pub fn json_array(items: impl IntoIterator<Item = String>) -> String {
    let values = items.into_iter().collect::<Vec<_>>().join(", ");
    format!("[{values}]")
}

pub fn json_object(fields: impl IntoIterator<Item = (String, String)>) -> String {
    let body = fields
        .into_iter()
        .map(|(key, value)| format!("{}: {}", quote(&key), value))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{body}}}")
}

pub fn string_set(values: impl IntoIterator<Item = String>) -> BTreeSet<String> {
    values.into_iter().collect()
}
