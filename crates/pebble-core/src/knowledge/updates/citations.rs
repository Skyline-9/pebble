//! Citation-token extraction and allow-list validation for proposed patches.

use std::collections::HashSet;

use super::AllowedCitation;
use crate::knowledge::KnowledgeError;

/// Reject a proposed patch body citing any repo/path/symbol outside `allowed`.
///
/// Recognizes citation tokens of the documented form
/// `` `repo@revision:path#symbol:Lstart-Lend` `` inside backticks; any other
/// backtick-quoted text is ignored.
pub(super) fn validate_citations(
    body: &str,
    allowed: &HashSet<AllowedCitation>,
) -> Result<(), KnowledgeError> {
    let mut rest = body;
    while let Some(start) = rest.find('`') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('`') else { break };
        if let Some(citation) = parse_citation(&after[..end])
            && !allowed.contains(&citation)
        {
            return Err(KnowledgeError::CitationNotAllowed(format!(
                "{}:{}#{}",
                citation.repo, citation.path, citation.symbol
            )));
        }
        rest = &after[end + 1..];
    }
    Ok(())
}

fn parse_citation(candidate: &str) -> Option<AllowedCitation> {
    let (repo_revision, path_symbol_line) = candidate.split_once(':')?;
    let (repo, _revision) = repo_revision.split_once('@')?;
    let (path, symbol_line) = path_symbol_line.split_once('#')?;
    let (symbol, _line) = symbol_line.split_once(':')?;
    if repo.is_empty() || path.is_empty() || symbol.is_empty() {
        return None;
    }
    Some(AllowedCitation {
        repo: repo.to_owned(),
        path: path.to_owned(),
        symbol: symbol.to_owned(),
    })
}
