use std::cmp::Ordering;
use std::iter::Peekable;
use std::path::Path;
use taplo::rowan::SyntaxElement;
use taplo::syntax::{SyntaxKind, SyntaxNode};

pub fn is_cargo_toml_file(file_path: &Path) -> bool {
    // don't need to worry about different casing because Cargo.toml will
    // always have this same casing https://github.com/rust-lang/cargo/issues/45
    file_path
        .file_name()
        .map(|n| n == "Cargo.toml")
        .unwrap_or(false)
}

pub fn apply_cargo_toml_conventions(node: SyntaxNode) -> SyntaxNode {
    let node = node.clone_for_update(); // use mutable API to make updates easier
    let mut children = node.children().peekable();

    while let Some(child) = children.next() {
        if child.text() == "[package]" {
            let mut package_section = Section::new(&child, &mut children);
            package_section.apply_formatting_conventions(sort_cargo_package_section);
            package_section.insert(&node);
        }
        if child.text() == "[dependencies]" || child.text() == "[dev-dependencies]" {
            let mut package_section = Section::new(&child, &mut children);
            package_section.apply_formatting_conventions(|left, right| {
                left.entry_key_text().cmp(&right.entry_key_text())
            });
            package_section.insert(&node);
        }
    }

    node
}

fn sort_cargo_package_section(left: &SyntaxNode, right: &SyntaxNode) -> Ordering {
    match (
        left.entry_key_text().as_str(),
        right.entry_key_text().as_str(),
    ) {
        ("name", _) => Ordering::Less,
        ("version", "name") => Ordering::Greater,
        ("version", _) => Ordering::Less,
        ("description", _) => Ordering::Greater,
        (_, "name") => Ordering::Greater,
        (_, "version") => Ordering::Greater,

        (left, right) => left.cmp(right),
    }
}

#[derive(Debug)]
struct Section {
    nodes: Vec<SyntaxNode>,
    table_header_index: usize,
}

impl Section {
    fn new(
        table_header: &SyntaxNode,
        tree: &mut Peekable<impl Iterator<Item = SyntaxNode>>,
    ) -> Self {
        let mut nodes = vec![];

        while let Some(entry) = tree.next_if(|child| child.kind() == SyntaxKind::ENTRY) {
            nodes.push(entry);
        }

        Self {
            nodes,
            table_header_index: table_header.index(),
        }
    }

    fn apply_formatting_conventions(
        &mut self,
        cmp: impl FnMut(&SyntaxNode, &SyntaxNode) -> Ordering,
    ) {
        self.nodes.sort_by(cmp);
    }

    fn insert(self, node: &SyntaxNode) {
        let start = self.table_header_index + 1;
        let end = start + self.nodes.len();

        node.splice_children(
            start..end,
            self.nodes.into_iter().map(SyntaxElement::Node).collect(),
        )
    }
}

trait SyntaxNodeExt {
    fn entry_key_text(&self) -> String;
}

impl SyntaxNodeExt for SyntaxNode {
    fn entry_key_text(&self) -> String {
        let key = self
            .children()
            .find(|child| child.kind() == SyntaxKind::KEY)
            .expect("ENTRY should contain KEY");

        let ident = key
            .children_with_tokens()
            .find_map(|child| match child {
                SyntaxElement::Token(token) if token.kind() == SyntaxKind::IDENT => Some(token),
                _ => None,
            })
            .expect("KEY should contain IDENT");

        ident.to_string()
    }
}
