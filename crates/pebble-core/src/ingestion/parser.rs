//! Iterative, one-tree-at-a-time packaged syntax extraction.

use std::collections::BTreeSet;

use tree_sitter::{Node, Parser};

use crate::domain::{RepositoryId, SymbolId};
use crate::repository::SourceFile;

use super::queries::{
    call_string_argument, call_target, import_target, is_call, is_declaration,
    is_declaration_identifier, is_identifier, is_import, node_name,
};
use super::{EdgeKind, Language, StructuralEdge, Symbol, edge};

pub(super) fn parse(
    repository: &RepositoryId,
    source: &SourceFile,
    language: Language,
) -> Option<(Vec<Symbol>, Vec<StructuralEdge>)> {
    let mut parser = Parser::new();
    parser.set_language(&language.grammar()).ok()?;
    let tree = parser.parse(source.contents(), None)?;
    if tree.root_node().has_error() {
        return None;
    }

    let mut extraction = Extraction {
        repository,
        source,
        language,
        declarations: BTreeSet::new(),
        symbols: Vec::new(),
        edges: Vec::new(),
    };
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        extraction.visit(node);
        let child_count = u32::try_from(node.child_count()).unwrap_or(u32::MAX);
        for index in (0..child_count).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }
    drop(tree);
    Some((extraction.symbols, extraction.edges))
}

struct Extraction<'source> {
    repository: &'source RepositoryId,
    source: &'source SourceFile,
    language: Language,
    declarations: BTreeSet<(usize, usize)>,
    symbols: Vec<Symbol>,
    edges: Vec<StructuralEdge>,
}

impl Extraction<'_> {
    fn visit(&mut self, node: Node<'_>) {
        if is_declaration(node.kind()) {
            self.extract_declaration(node);
        }
        if is_import(node.kind()) {
            if let Some(target) = import_target(node)
                .and_then(|target| text(target, self.source.contents()))
                .map(semantic_target)
                .filter(|target| !target.is_empty())
            {
                self.edges.push(edge(
                    EdgeKind::Imports,
                    self.source.id(),
                    target.to_owned(),
                    line(node),
                ));
            }
        } else if is_call(node.kind()) {
            self.extract_call(node);
        }
        if is_identifier(node.kind())
            && !self
                .declarations
                .contains(&(node.start_byte(), node.end_byte()))
            && !is_declaration_identifier(node)
            && let Some(target) = text(node, self.source.contents())
        {
            self.edges.push(edge(
                EdgeKind::References,
                self.source.id(),
                target.to_owned(),
                line(node),
            ));
        }
    }

    fn extract_declaration(&mut self, node: Node<'_>) {
        let Some(name_node) = node_name(node) else {
            return;
        };
        let Some(name) = text(name_node, self.source.contents()).map(str::to_owned) else {
            return;
        };
        self.declarations
            .insert((name_node.start_byte(), name_node.end_byte()));
        let start_line = line(node);
        let end_line = end_line(node);
        let semantic_name = format!(
            "{}:{name}:{}-{}",
            self.source.path(),
            name_node.start_byte(),
            name_node.end_byte()
        );
        let id = SymbolId::derive(self.repository, self.language.name(), &semantic_name);
        self.edges.push(edge(
            EdgeKind::Defines,
            self.source.id(),
            id.as_str().to_owned(),
            start_line,
        ));
        self.symbols.push(Symbol {
            id,
            name,
            start_line,
            end_line,
        });
    }

    fn extract_call(&mut self, node: Node<'_>) {
        let Some(target_node) = call_target(node) else {
            return;
        };
        let Some(target) = text(target_node, self.source.contents()) else {
            return;
        };
        if target == "require" {
            if let Some(module) = call_string_argument(node)
                .and_then(|argument| text(argument, self.source.contents()))
                .map(semantic_target)
                .filter(|module| !module.is_empty())
            {
                self.edges.push(edge(
                    EdgeKind::Imports,
                    self.source.id(),
                    module.to_owned(),
                    line(node),
                ));
            }
            return;
        }
        self.edges.push(edge(
            EdgeKind::Calls,
            self.source.id(),
            target.to_owned(),
            line(node),
        ));
    }
}

fn text<'source>(node: Node<'_>, source: &'source str) -> Option<&'source str> {
    source.get(node.byte_range())
}

fn line(node: Node<'_>) -> u32 {
    u32::try_from(node.start_position().row).map_or(u32::MAX, |row| row.saturating_add(1))
}

fn end_line(node: Node<'_>) -> u32 {
    u32::try_from(node.end_position().row).map_or(u32::MAX, |row| row.saturating_add(1))
}

fn semantic_target(target: &str) -> &str {
    target
        .trim()
        .trim_matches(|character| matches!(character, '"' | '\'' | '`' | '<' | '>'))
}
