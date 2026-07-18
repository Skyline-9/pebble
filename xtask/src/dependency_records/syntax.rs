pub(super) fn contains_unicode_escape(value: &str) -> bool {
    value.contains("\\u") || value.contains("\\U")
}

pub(super) fn without_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if let Some(delimiter) = quote {
            if character == delimiter && !escaped {
                quote = None;
            }
            escaped = character == '\\' && !escaped;
            if character != '\\' {
                escaped = false;
            }
        } else if matches!(character, '"' | '\'') {
            quote = Some(character);
        } else if character == '#' {
            return &line[..index];
        }
    }
    line
}
