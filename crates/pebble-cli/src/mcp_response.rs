//! MCP tool response shaping and full-frame budget enforcement.

use rmcp::model::{
    CallToolResult, ErrorData, JsonRpcMessage, RequestId, ServerJsonRpcMessage, ServerResult,
};
use serde_json::Value;

pub fn success(
    mut value: Value,
    id: &RequestId,
    budget: Option<u32>,
) -> Result<CallToolResult, ErrorData> {
    if let Some(budget) = budget {
        fit_search_value(&mut value, id, budget)?;
    }
    Ok(structured(value, false))
}

pub fn structured_error(value: Value) -> CallToolResult {
    structured(value, true)
}

fn structured(value: Value, is_error: bool) -> CallToolResult {
    let mut result = if is_error {
        CallToolResult::error(Vec::new())
    } else {
        CallToolResult::success(Vec::new())
    };
    result.structured_content = Some(value);
    result
}

fn fit_search_value(value: &mut Value, id: &RequestId, budget: u32) -> Result<(), ErrorData> {
    let maximum = usize::try_from(budget).unwrap_or(usize::MAX);
    loop {
        let length = frame_length(value, id)?;
        if length <= maximum {
            return Ok(());
        }
        let excess = length.saturating_sub(maximum);
        if trim_one_excerpt(value, excess) || remove_one_item(value) || remove_one_diagnostic(value)
        {
            continue;
        }
        return Err(ErrorData::internal_error(
            "search response envelope exceeds requested budget",
            None,
        ));
    }
}

fn frame_length(value: &Value, id: &RequestId) -> Result<usize, ErrorData> {
    let response: ServerJsonRpcMessage = JsonRpcMessage::response(
        ServerResult::CallToolResult(structured(value.clone(), false)),
        id.clone(),
    );
    serde_json::to_vec(&response)
        .map(|bytes| bytes.len().saturating_add(1))
        .map_err(|error| ErrorData::internal_error(error.to_string(), None))
}

fn trim_one_excerpt(value: &mut Value, excess: usize) -> bool {
    let Some(items) = value.get_mut("items").and_then(Value::as_array_mut) else {
        return false;
    };
    for item in items.iter_mut().rev() {
        let Some(content) = item.get_mut("content") else {
            continue;
        };
        let Some(text) = content.as_str() else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        let mut target = text.len().saturating_sub(excess.max(1));
        while !text.is_char_boundary(target) {
            target = target.saturating_sub(1);
        }
        let shortened = text[..target].trim_end().to_owned();
        if shortened.is_empty() {
            continue;
        }
        let lines = u32::try_from(shortened.lines().count()).unwrap_or(u32::MAX);
        *content = Value::String(shortened);
        if let Some(citation) = item.get_mut("citation") {
            let start = citation
                .get("start_line")
                .and_then(Value::as_u64)
                .unwrap_or(1);
            citation["end_line"] =
                Value::from(start.saturating_add(u64::from(lines.saturating_sub(1))));
        }
        return true;
    }
    false
}

fn remove_one_item(value: &mut Value) -> bool {
    value
        .get_mut("items")
        .and_then(Value::as_array_mut)
        .is_some_and(|items| items.pop().is_some())
}

fn remove_one_diagnostic(value: &mut Value) -> bool {
    value
        .get_mut("diagnostics")
        .and_then(Value::as_array_mut)
        .is_some_and(|items| items.pop().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_response_frame_is_fitted_without_duplicate_content()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = RequestId::Number(7);
        let mut value = serde_json::json!({
            "budget_tokens": 1000,
            "items": [{
                "citation": {"start_line": 1, "end_line": 1},
                "content": "x".repeat(2000),
                "score_explanations": []
            }],
            "diagnostics": []
        });
        fit_search_value(&mut value, &id, 1000)?;
        assert!(frame_length(&value, &id).is_ok_and(|length| length <= 1000));
        let result = structured(value, false);
        assert!(result.content.is_empty());
        assert!(result.structured_content.is_some());
        Ok(())
    }
}
