//! Conservative node classifications shared by packaged grammars.

use tree_sitter::Node;

pub(super) fn is_declaration(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition"
            | "function_declaration"
            | "function_item"
            | "method"
            | "method_declaration"
            | "method_definition"
            | "constructor_declaration"
            | "class_declaration"
            | "class_definition"
            | "class_specifier"
            | "interface_declaration"
            | "struct_item"
            | "struct_specifier"
            | "enum_item"
            | "enum_declaration"
            | "type_declaration"
    )
}

pub(super) fn is_import(kind: &str) -> bool {
    matches!(
        kind,
        "import"
            | "import_statement"
            | "import_from_statement"
            | "future_import_statement"
            | "import_declaration"
            | "import_spec"
            | "use_declaration"
            | "using_directive"
            | "using_declaration"
            | "preproc_include"
    )
}

pub(super) fn is_call(kind: &str) -> bool {
    matches!(
        kind,
        "call"
            | "call_expression"
            | "invocation_expression"
            | "method_invocation"
            | "macro_invocation"
    )
}

pub(super) fn is_identifier(kind: &str) -> bool {
    kind == "identifier"
        || kind == "type_identifier"
        || kind == "simple_identifier"
        || kind.ends_with("_identifier")
}

pub(super) fn node_name(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("name")
        .or_else(|| node.child_by_field_name("declarator"))
        .and_then(first_identifier)
        .or_else(|| first_identifier(node))
}

pub(super) fn import_target(node: Node<'_>) -> Option<Node<'_>> {
    match node.kind() {
        "import_statement" => node
            .child_by_field_name("source")
            .or_else(|| node.child_by_field_name("name")),
        "import_from_statement" | "future_import_statement" => {
            node.child_by_field_name("module_name")
        }
        "import_spec" | "preproc_include" => node.child_by_field_name("path"),
        "use_declaration" => node.child_by_field_name("argument"),
        "import_declaration" if first_descendant(node, "import_spec").is_some() => None,
        "using_directive" => u32::try_from(node.named_child_count())
            .ok()
            .and_then(|count| count.checked_sub(1))
            .and_then(|index| node.named_child(index))
            .or_else(|| node.child_by_field_name("name")),
        _ => node.named_child(0),
    }
}

pub(super) fn call_target(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("method")
        .or_else(|| node.child_by_field_name("name"))
        .and_then(first_identifier)
        .or_else(|| {
            let function = node.child_by_field_name("function")?;
            ["method", "name", "property", "field", "attribute", "member"]
                .into_iter()
                .find_map(|field| function.child_by_field_name(field))
                .and_then(first_identifier)
                .or_else(|| last_identifier(function))
        })
        .or_else(|| node.named_child(0).and_then(last_identifier))
}

pub(super) fn call_string_argument(node: Node<'_>) -> Option<Node<'_>> {
    let arguments = node.child_by_field_name("arguments")?;
    first_string(arguments)
}

pub(super) fn is_declaration_identifier(node: Node<'_>) -> bool {
    let mut child = node;
    while let Some(parent) = child.parent() {
        if is_import(parent.kind()) || parent.kind() == "package_clause" {
            return true;
        }
        if is_declaration(parent.kind()) {
            return !within_field(parent, node, "body") && !is_body_container(child.kind());
        }
        if is_local_declaration(parent.kind()) {
            return !["value", "right", "body"]
                .into_iter()
                .any(|field| within_field(parent, node, field));
        }
        if is_parameter(parent.kind()) {
            return true;
        }
        child = parent;
    }
    false
}

fn first_identifier(root: Node<'_>) -> Option<Node<'_>> {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if is_identifier(node.kind()) {
            return Some(node);
        }
        let child_count = u32::try_from(node.child_count()).unwrap_or(u32::MAX);
        for index in (0..child_count).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }
    None
}

fn last_identifier(root: Node<'_>) -> Option<Node<'_>> {
    let mut found = None;
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if is_identifier(node.kind()) {
            found = Some(node);
        }
        let child_count = u32::try_from(node.child_count()).unwrap_or(u32::MAX);
        for index in (0..child_count).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }
    found
}

fn first_descendant<'tree>(root: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node != root && node.kind() == kind {
            return Some(node);
        }
        let child_count = u32::try_from(node.child_count()).unwrap_or(u32::MAX);
        for index in (0..child_count).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }
    None
}

fn first_string(root: Node<'_>) -> Option<Node<'_>> {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if matches!(
            node.kind(),
            "string" | "string_literal" | "interpreted_string_literal" | "raw_string_literal"
        ) {
            return Some(node);
        }
        let child_count = u32::try_from(node.child_count()).unwrap_or(u32::MAX);
        for index in (0..child_count).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }
    None
}

fn within_field(parent: Node<'_>, node: Node<'_>, field: &str) -> bool {
    parent.child_by_field_name(field).is_some_and(|candidate| {
        candidate.start_byte() <= node.start_byte() && candidate.end_byte() >= node.end_byte()
    })
}

fn is_local_declaration(kind: &str) -> bool {
    matches!(
        kind,
        "assignment"
            | "declaration"
            | "init_declarator"
            | "lexical_declaration"
            | "short_var_declaration"
            | "variable_declaration"
            | "variable_declarator"
            | "var_spec"
    )
}

fn is_parameter(kind: &str) -> bool {
    kind == "parameter"
        || kind == "parameters"
        || kind == "formal_parameters"
        || kind == "parameter_list"
        || kind.ends_with("_parameter")
        || kind.ends_with("_parameter_declaration")
}

fn is_body_container(kind: &str) -> bool {
    matches!(kind, "block" | "compound_statement" | "statement_block")
        || kind.ends_with("_body")
        || kind.ends_with("_block")
}
