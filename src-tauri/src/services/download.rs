use anyhow::Error;

/// Formats download failures for UI without exposing credentials or URL query data.
pub fn diagnostic(error: &Error) -> String {
    let mut messages = Vec::new();
    for cause in error.chain() {
        let message = sanitize(&cause.to_string());
        if !message.is_empty() && messages.last() != Some(&message) {
            messages.push(message);
        }
    }
    messages.join("; ")
}

fn sanitize(message: &str) -> String {
    let mut sanitized = message.to_string();
    let mut search_from = 0;
    while let Some(relative) = sanitized[search_from..].find("://") {
        let marker = search_from + relative;
        let scheme_start = sanitized[..marker]
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            .last()
            .map(|(index, _)| index)
            .unwrap_or(marker);
        let scheme = &sanitized[scheme_start..marker];
        if scheme.is_empty() || !scheme.chars().next().unwrap().is_ascii_alphabetic() {
            search_from = marker + 3;
            continue;
        }
        let start = scheme_start;
        let end = sanitized[start..]
            .find(|c: char| {
                c.is_whitespace() || matches!(c, ')' | ']' | '}' | ',' | ';' | '"' | '\'')
            })
            .map(|offset| start + offset)
            .unwrap_or(sanitized.len());
        if let Ok(mut url) = url::Url::parse(&sanitized[start..end]) {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            let replacement = url.to_string().trim_end_matches('/').to_owned();
            sanitized.replace_range(start..end, &replacement);
            search_from = start + replacement.len();
        } else {
            search_from = end;
        }
    }
    redact_credential_values(&sanitized)
}

fn redact_credential_values(message: &str) -> String {
    const KEYS: &[&str] = &[
        "password",
        "passwd",
        "token",
        "access_token",
        "api_key",
        "client_secret",
        "secret",
        "authorization",
        "bearer",
    ];
    let mut output = String::with_capacity(message.len());
    let mut cursor = 0;
    while cursor < message.len() {
        let lower = message[cursor..].to_ascii_lowercase();
        let Some((key_start, key)) = KEYS
            .iter()
            .filter_map(|key| lower.find(key).map(|offset| (cursor + offset, *key)))
            .min_by_key(|(start, _)| *start)
        else {
            output.push_str(&message[cursor..]);
            break;
        };
        let key_end = key_start + key.len();
        let boundary = key_start == 0
            || !message.as_bytes()[key_start - 1].is_ascii_alphanumeric()
                && message.as_bytes()[key_start - 1] != b'_';
        if !boundary || key_end >= message.len() {
            output.push_str(&message[cursor..key_end.min(message.len())]);
            cursor = key_end.min(message.len());
            continue;
        }
        let mut value_start = key_end;
        while value_start < message.len() && message.as_bytes()[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        if value_start >= message.len() || !matches!(message.as_bytes()[value_start], b'=' | b':') {
            output.push_str(&message[cursor..key_end]);
            cursor = key_end;
            continue;
        }
        value_start += 1;
        while value_start < message.len() && message.as_bytes()[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        if message[value_start..]
            .to_ascii_lowercase()
            .starts_with("bearer ")
        {
            value_start += "bearer ".len();
        }
        let value_end = message[value_start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '&' | ',' | ';' | ')' | ']' | '}'))
            .map(|offset| value_start + offset)
            .unwrap_or(message.len());
        output.push_str(&message[cursor..value_start]);
        output.push_str("[redacted]");
        cursor = value_end;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::sanitize;

    #[test]
    fn sanitizes_arbitrary_scheme_userinfo_and_query() {
        let result = sanitize("request socks5://user:pass@example.test/path?token=secret&x=y");
        assert_eq!(result, "request socks5://example.test/path");
    }

    #[test]
    fn redacts_credential_keys_case_insensitively() {
        let result = sanitize("PASSWORD=one; Access_Token: two, Authorization: Bearer three");
        assert_eq!(
            result,
            "PASSWORD=[redacted]; Access_Token: [redacted], Authorization: Bearer [redacted]"
        );
    }
}
