//! Boundary-aware contradiction detection for emitted evidence.

use std::collections::BTreeSet;

pub(super) fn detect<'content>(contents: impl Iterator<Item = &'content str>) -> bool {
    let documents = contents
        .map(|content| {
            let tokens = content
                .split(|character: char| !character.is_alphanumeric())
                .filter(|token| !token.is_empty())
                .map(str::to_lowercase)
                .collect::<Vec<_>>();
            let positive = tokens.iter().enumerate().any(|(index, token)| {
                matches!(token.as_str(), "enabled" | "true")
                    || (matches!(token.as_str(), "support" | "supports")
                        && index
                            .checked_sub(1)
                            .is_none_or(|before| tokens[before] != "not"))
                    || (token == "must"
                        && tokens
                            .get(index.saturating_add(1))
                            .is_none_or(|next| next != "not"))
            });
            let negative = tokens.iter().enumerate().any(|(index, token)| {
                matches!(token.as_str(), "disabled" | "false")
                    || (matches!(token.as_str(), "support" | "supports")
                        && index
                            .checked_sub(1)
                            .is_some_and(|before| tokens[before] == "not"))
                    || (token == "must"
                        && tokens
                            .get(index.saturating_add(1))
                            .is_some_and(|next| next == "not"))
            });
            let context = tokens
                .into_iter()
                .filter(|token| {
                    !matches!(
                        token.as_str(),
                        "enabled"
                            | "disabled"
                            | "true"
                            | "false"
                            | "support"
                            | "supports"
                            | "supported"
                            | "must"
                            | "not"
                            | "no"
                            | "never"
                            | "do"
                            | "does"
                    )
                })
                .collect::<BTreeSet<_>>();
            (positive, negative, context)
        })
        .collect::<Vec<_>>();
    documents.iter().enumerate().any(|(left_index, left)| {
        documents
            .iter()
            .enumerate()
            .skip(left_index.saturating_add(1))
            .any(|(_, right)| {
                ((left.0 && right.1) || (left.1 && right.0))
                    && strong_context_overlap(&left.2, &right.2)
            })
    })
}

fn strong_context_overlap(left: &BTreeSet<String>, right: &BTreeSet<String>) -> bool {
    if left.is_empty() || right.is_empty() {
        return false;
    }
    let shared = left.intersection(right).count();
    shared.saturating_mul(4) >= left.len().max(right.len()).saturating_mul(3)
}

#[cfg(test)]
mod tests {
    use super::detect;

    #[test]
    fn detects_each_supported_negation_at_token_boundaries() {
        assert!(detect(
            ["audit feature enabled", "audit feature disabled"].into_iter()
        ));
        assert!(detect(
            ["audit value is true", "audit value is false"].into_iter()
        ));
        assert!(detect(
            ["service supports Linux", "service does not support Linux"].into_iter()
        ));
        assert!(!detect(
            [
                "operators must rotate keys",
                "operators must not reuse keys"
            ]
            .into_iter()
        ));
        assert!(!detect(
            ["reenabled", "disabledness", "supportive", "mustard"].into_iter()
        ));
    }
}
