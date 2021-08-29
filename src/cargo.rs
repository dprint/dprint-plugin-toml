use std::cmp::Ordering;
use std::iter::Peekable;
use std::path::Path;
use taplo::{
  rowan::NodeOrToken,
  syntax::{SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken},
};

use crate::rowan_extensions::*;

pub fn is_cargo_toml_file(file_path: &Path) -> bool {
  // don't need to worry about different casing because Cargo.toml will
  // always have this same casing https://github.com/rust-lang/cargo/issues/45
  file_path.file_name().map(|n| n == "Cargo.toml").unwrap_or(false)
}

pub fn apply_cargo_toml_conventions(node: SyntaxNode) -> SyntaxNode {
  let node = node.clone_for_update(); // use mutable API to make updates easier
  let mut children = node.children().peekable();

  while let Some(child) = children.next() {
    if child.kind() == SyntaxKind::TABLE_HEADER {
      if child.text() == "[package]" {
        let section_children = get_section_children(&mut children);
        sort_nodes(&node, section_children, &sort_cargo_package_section);
      }
      if child.text() == "[dependencies]" || child.text() == "[dev-dependencies]" {
        let section_children = get_section_children(&mut children);
        sort_nodes(&node, section_children, &|left, right| left.entry_key_text().cmp(&right.entry_key_text()));
      }
    }
  }

  node
}

fn sort_cargo_package_section(left: &SyntaxNode, right: &SyntaxNode) -> Ordering {
  match (left.entry_key_text().as_str(), right.entry_key_text().as_str()) {
    ("version", "name") => Ordering::Greater,
    ("name", _) => Ordering::Less,
    ("version", _) => Ordering::Less,
    ("description", _) => Ordering::Greater,
    (_, "name") => Ordering::Greater,
    (_, "version") => Ordering::Greater,
    (_, "description") => Ordering::Less,

    (left, right) => left.cmp(right),
  }
}

fn get_section_children(children: &mut Peekable<impl Iterator<Item = SyntaxNode>>) -> Vec<SyntaxNode> {
  let mut nodes = vec![];

  while let Some(entry) = children.next_if(|child| child.kind() == SyntaxKind::ENTRY) {
    nodes.push(entry);
  }

  nodes
}

fn sort_nodes(parent: &SyntaxNode, children: Vec<SyntaxNode>, cmp: &impl Fn(&SyntaxNode, &SyntaxNode) -> Ordering) {
  if children.is_empty() {
    return; // nothing to do
  }

  let children: Vec<_> = children.into_iter().map(NodeWithLeadingTrivia::from).collect();
  let start = children.first().unwrap().start_index();
  let end = children.last().unwrap().end_index();
  let children = sort_children(children, cmp);

  let mut nodes_and_comments: Vec<SyntaxElement> = Vec::new();

  for node in children.into_iter() {
    nodes_and_comments.extend(node.trivia.into_iter().map(|c| c.into()));
    nodes_and_comments.push(node.node.into());
  }

  parent.splice_children(start..end, nodes_and_comments)
}

struct NodeWithLeadingTrivia {
  node: SyntaxNode,
  trivia: Vec<SyntaxToken>,
}

impl NodeWithLeadingTrivia {
  pub fn from(node: SyntaxNode) -> Self {
    let trivia = node.get_previous_trivia();
    NodeWithLeadingTrivia { node, trivia }
  }

  pub fn start_index(&self) -> usize {
    self.trivia.first().map(|t| t.index()).unwrap_or_else(|| self.node.index())
  }

  pub fn end_index(&self) -> usize {
    self.node.index() + 1
  }
}

fn sort_children(children: Vec<NodeWithLeadingTrivia>, cmp: &impl Fn(&SyntaxNode, &SyntaxNode) -> Ordering) -> Vec<NodeWithLeadingTrivia> {
  // Break up the children into groups based on if they have a preceeding blank line.
  // This allows people to explicitly define "groups" via blank lines within the children.
  let mut child_groups: Vec<Vec<NodeWithLeadingTrivia>> = Vec::new();
  for child in children.into_iter() {
    if child_groups.is_empty() || child.node.has_previous_blank_line() {
      let child_group = Vec::new();
      child_groups.push(child_group);
    }
    child_groups.last_mut().unwrap().push(child);
  }

  // sort each group
  for child_group in child_groups.iter_mut() {
    sort_group(child_group, cmp);
  }

  child_groups.into_iter().flatten().collect()
}

fn sort_group(child_group: &mut Vec<NodeWithLeadingTrivia>, cmp: &impl Fn(&SyntaxNode, &SyntaxNode) -> Ordering) {
  // remove the first item's trivia to get the group trivia
  let group_trivia: Vec<_> = child_group.first_mut().unwrap().trivia.drain(..).collect();
  let previous_first_item_index = child_group.first().unwrap().node.index();

  // sort the items
  child_group.sort_by(|left, right| cmp(&left.node, &right.node));

  // now insert the group trivia to the new first item and
  // take the new first item and make its non-comment leading
  // trivia the previous first item's trivia
  let first_item_trivia_to_move = {
    let first_item = child_group.first_mut().unwrap();
    let trivia_remove_end = first_item
      .trivia
      .iter()
      .position(|t| t.kind() == SyntaxKind::COMMENT)
      .unwrap_or(first_item.trivia.len());
    let trivia = first_item.trivia.drain(..trivia_remove_end).collect::<Vec<_>>();
    first_item.trivia.splice(0..0, group_trivia);
    trivia
  };
  let previous_first_item = child_group.iter_mut().find(|t| t.node.index() == previous_first_item_index).unwrap();
  previous_first_item.trivia.extend(first_item_trivia_to_move);
}

trait SyntaxNodeExt {
  fn entry_key_text(&self) -> String;
  fn has_previous_blank_line(&self) -> bool;
  fn get_previous_trivia(&self) -> Vec<SyntaxToken>;
}

impl SyntaxNodeExt for SyntaxNode {
  fn entry_key_text(&self) -> String {
    let key = self.children().find(|child| child.kind() == SyntaxKind::KEY).expect("ENTRY should contain KEY");

    let ident = key
      .children_with_tokens()
      .find_map(|child| match child {
        SyntaxElement::Token(token) if token.kind() == SyntaxKind::IDENT => Some(token),
        _ => None,
      })
      .expect("KEY should contain IDENT");

    ident.to_string()
  }

  fn has_previous_blank_line(&self) -> bool {
    let mut element: SyntaxElement = self.clone().into();
    let mut last_was_newline = false;
    while let Some(sibling) = element.prev_sibling_or_token() {
      element = sibling.clone();
      match sibling {
        NodeOrToken::Token(token) => match token.kind() {
          SyntaxKind::COMMENT => last_was_newline = false,
          SyntaxKind::NEWLINE => {
            if last_was_newline || token.newline_count() > 1 {
              return true;
            }
            last_was_newline = true;
          }
          SyntaxKind::WHITESPACE => continue,
          _ => return false,
        },
        NodeOrToken::Node(_) => return false,
      }
    }

    false
  }

  fn get_previous_trivia(&self) -> Vec<SyntaxToken> {
    let mut trivia = Vec::new();
    let mut element: SyntaxElement = self.clone().into();
    while let Some(sibling) = element.prev_sibling_or_token() {
      element = sibling.clone();
      match sibling {
        NodeOrToken::Token(token) => match token.kind() {
          SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT => trivia.push(token),
          _ => break,
        },
        NodeOrToken::Node(_) => break,
      }
    }
    trivia.reverse();
    trivia
  }
}
