#![allow(clippy::needless_lifetimes)]

use dprint_core::formatting::conditions::*;
use dprint_core::formatting::ir_helpers::SingleLineOptions;
use dprint_core::formatting::*;
use std::cell::Cell;
use std::rc::Rc;
use taplo::rowan::NodeOrToken;
use taplo::syntax::SyntaxElement;
use taplo::syntax::SyntaxKind;
use taplo::syntax::SyntaxNode;
use taplo::syntax::SyntaxToken;

use super::Context;
use crate::configuration::Configuration;
use crate::rowan_extensions::*;

type PrintItemsResult = Result<PrintItems, ()>;

pub fn generate(node: SyntaxNode, text: &str, config: &Configuration) -> PrintItems {
  let mut context = Context::new(text, config);
  let mut items = gen_node(node.into(), &mut context);
  items.push_condition(if_true(
    "endOfFileNewLine",
    Rc::new(|context| Some(context.writer_info.column_number > 0 || context.writer_info.line_number > 0)),
    Signal::NewLine.into(),
  ));
  items
}

fn gen_node<'a>(node: SyntaxElement, context: &mut Context<'a>) -> PrintItems {
  gen_node_with_inner(node, context, |items, _| items)
}

fn gen_node_with_inner<'a>(node: SyntaxElement, context: &mut Context<'a>, inner_parse: impl FnOnce(PrintItems, &mut Context<'a>) -> PrintItems) -> PrintItems {
  let mut items = PrintItems::new();
  // println!("{:?}", node);

  if node.kind() != SyntaxKind::COMMENT {
    for comment in node.get_comments_on_previous_lines() {
      if !context.has_handled_comment(comment.text_range().start().into()) {
        items.extend(gen_comment(comment.clone(), context));
        items.push_signal(Signal::NewLine);
        if NodeOrToken::Token(comment).has_trailing_blank_line() {
          items.push_signal(Signal::NewLine);
        }
      }
    }
  }

  let result = match node.clone() {
    NodeOrToken::Node(node) => match node.kind() {
      SyntaxKind::ROOT => gen_root(node, context),
      SyntaxKind::ARRAY => gen_array(node, context),
      SyntaxKind::INLINE_TABLE => gen_inline_table(node, context),
      SyntaxKind::ENTRY => gen_entry(node, context),
      SyntaxKind::KEY => gen_key(node, context),
      SyntaxKind::VALUE => gen_value(node, context),
      SyntaxKind::TABLE_HEADER => gen_table_header(node, context),
      SyntaxKind::TABLE_ARRAY_HEADER => gen_table_array_header(node, context),
      _ => Err(()),
    },
    NodeOrToken::Token(token) => match token.kind() {
      SyntaxKind::COMMENT => Ok(gen_comment(token, context)),
      SyntaxKind::MULTI_LINE_STRING | SyntaxKind::MULTI_LINE_STRING_LITERAL => {
        let mut items = PrintItems::new();
        items.push_str("");
        items.extend(ir_helpers::gen_from_raw_string(token.text().trim()));
        Ok(items)
      }
      _ => Ok(ir_helpers::gen_from_string(token.text().trim())),
    },
  };

  items.extend(inner_parse(
    match result {
      Ok(items) => items,
      Err(()) => ir_helpers::gen_from_raw_string_trim_line_ends(node.text().trim()),
    },
    context,
  ));

  if matches!(node.kind(), SyntaxKind::VALUE | SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER) {
    for comment in node.child_comments() {
      items.extend(gen_comment(comment, context));
    }
  }
  if node.parent().is_none() || node.parent().unwrap().kind() != SyntaxKind::VALUE || !node.is_last_non_trivia_sibling() {
    if let Some(trailing_comment) = get_trailing_comment(node) {
      items.extend(gen_comment(trailing_comment, context));
    }
  }

  items
}

fn gen_root<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
  // print_formatted_tree(node.clone());

  let newline_count = Rc::new(Cell::new(0));
  let mut gen_element = {
    let mut found_first = false;
    let mut last_node_kind = None;
    let new_line_count = newline_count.clone();
    move |element: SyntaxElement| {
      let mut items = PrintItems::new();
      if found_first {
        items.push_signal(Signal::NewLine);
        if new_line_count.get() > 1 && allow_blank_line(last_node_kind, element.kind()) {
          items.push_signal(Signal::NewLine);
        }
      }

      last_node_kind = Some(element.kind());
      items.extend(gen_node(element, context));

      found_first = true;
      new_line_count.set(0);
      items
    }
  };

  let mut items = PrintItems::new();
  for element in node.children_with_tokens() {
    match element {
      NodeOrToken::Node(_) => items.extend(gen_element(element)),
      NodeOrToken::Token(token) => match token.kind() {
        SyntaxKind::NEWLINE => newline_count.set(newline_count.get() + token.newline_count()),
        SyntaxKind::COMMENT => items.extend(gen_element(token.into())),
        _ => {}
      },
    }
  }

  Ok(items)
}

fn allow_blank_line(previous_kind: Option<SyntaxKind>, current_kind: SyntaxKind) -> bool {
  if matches!(current_kind, SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER) {
    true
  } else {
    !matches!(previous_kind, Some(SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER))
  }
}

fn gen_array<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
  let values = node.children();
  let open_token = get_token_with_kind(node.clone(), SyntaxKind::BRACKET_START)?;
  let close_token = get_token_with_kind(node.clone(), SyntaxKind::BRACKET_END)?;
  let is_in_inline_table = node.ancestors().any(|a| a.kind() == SyntaxKind::INLINE_TABLE);
  let force_use_new_lines = !is_in_inline_table && has_following_newline(open_token.clone());
  ensure_all_kind(values.clone(), SyntaxKind::VALUE)?;

  Ok(gen_surrounded_by_tokens(
    |context| {
      gen_comma_separated_values(
        ParseCommaSeparatedValuesOptions {
          nodes: values.into_iter().map(|v| v.into()).collect::<Vec<_>>(),
          prefer_hanging: false,
          force_use_new_lines,
          allow_blank_lines: true,
          single_line_space_at_start: false,
          single_line_space_at_end: false,
          custom_single_line_separator: None,
          multi_line_options: ir_helpers::MultiLineOptions::surround_newlines_indented(),
          force_possible_newline_at_start: false,
        },
        context,
      )
    },
    ParseSurroundedByTokensParams { open_token, close_token },
    context,
  ))
}

fn gen_inline_table<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
  let values = node.children();
  ensure_all_kind(values.clone(), SyntaxKind::ENTRY)?;

  let mut items = PrintItems::new();
  items.push_str("{");
  let mut had_item = false;
  for (i, value) in values.enumerate() {
    items.push_str(if i > 0 { ", " } else { " " });
    items.extend(gen_node(value.into(), context));
    had_item = true;
  }
  items.push_str(if had_item { " }" } else { "}" });

  // the comment seems to be stored as the last child of an inline table, so check for it here
  if let Some(NodeOrToken::Token(token)) = node.children_with_tokens().last() {
    if token.kind() == SyntaxKind::COMMENT {
      items.extend(gen_comment(token, context));
    }
  }

  // Disable newlines in a table. The spec says the following:
  // > Inline tables are intended to appear on a single line. A terminating comma (also called trailing comma)
  // > is not permitted after the last key/value pair in an inline table. No newlines are allowed between the
  // > curly braces unless they are valid within a value. Even so, it is strongly discouraged to break an inline
  // > table onto multiples lines. If you find yourself gripped with this desire, it means you should be using
  // > standard tables.
  Ok(ir_helpers::with_no_new_lines(items))
}

fn gen_entry<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
  let key = get_child_with_kind(node.clone(), SyntaxKind::KEY)?;
  let value = get_child_with_kind(node.clone(), SyntaxKind::VALUE)?;
  let mut items = PrintItems::new();

  items.extend(gen_node(key.into(), context));
  items.push_str(" = ");
  items.extend(gen_node(value.into(), context));

  Ok(items)
}

fn gen_key<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
  // Spec: A key may be either bare, quoted, or dotted.
  Ok(gen_children_inline(node, context))
}

fn gen_value<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
  // Spec: Values must be either String, Integer, Float, Boolean, DateTimes, Array, InlineTable
  Ok(gen_children_inline(node, context))
}

fn gen_children_inline<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItems {
  let mut items = PrintItems::new();
  for element in get_children_with_non_trivia_tokens(node) {
    items.extend(gen_node(element, context));
  }
  items
}

fn gen_table_header<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
  // Spec: Naming rules for tables are the same as for keys
  let key = get_child_with_kind(node.clone(), SyntaxKind::KEY)?;
  let mut items = PrintItems::new();
  items.push_str("[");
  items.extend(gen_node(key.into(), context));
  items.push_str("]");
  Ok(items)
}

fn gen_table_array_header<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
  // Spec: Naming rules for tables are the same as for keys
  let key = get_child_with_kind(node.clone(), SyntaxKind::KEY)?;
  let mut items = PrintItems::new();
  items.push_str("[[");
  items.extend(gen_node(key.into(), context));
  items.push_str("]]");
  Ok(items)
}

struct ParseSurroundedByTokensParams {
  open_token: SyntaxToken,
  close_token: SyntaxToken,
}

fn gen_surrounded_by_tokens<'a, 'b>(
  gen_inner: impl FnOnce(&mut Context<'a>) -> PrintItems,
  opts: ParseSurroundedByTokensParams,
  context: &mut Context<'a>,
) -> PrintItems {
  // parse
  let mut items = PrintItems::new();
  items.extend(gen_node(opts.open_token.clone().into(), context));

  items.extend(gen_inner(context));

  let close_token: SyntaxElement = opts.close_token.into();
  for comment in close_token.get_comments_on_previous_lines() {
    if NodeOrToken::Token(comment.clone()).has_leading_blank_line() {
      items.push_signal(Signal::NewLine);
    }
    items.extend(ir_helpers::with_indent(gen_comment(comment, context)));
    items.push_signal(Signal::NewLine);
  }

  items.extend(gen_node(close_token, context));
  items
}

struct ParseCommaSeparatedValuesOptions {
  nodes: Vec<SyntaxElement>,
  prefer_hanging: bool,
  force_use_new_lines: bool,
  allow_blank_lines: bool,
  single_line_space_at_start: bool,
  single_line_space_at_end: bool,
  custom_single_line_separator: Option<PrintItems>,
  multi_line_options: ir_helpers::MultiLineOptions,
  force_possible_newline_at_start: bool,
}

fn gen_comma_separated_values<'a>(opts: ParseCommaSeparatedValuesOptions, context: &mut Context<'a>) -> PrintItems {
  let nodes = opts.nodes;
  let indent_width = context.config.indent_width;
  let compute_lines_span = opts.allow_blank_lines; // save time otherwise
  ir_helpers::gen_separated_values(
    |is_multi_line_ref| {
      let mut generated_nodes = Vec::new();
      let nodes_count = nodes.len();
      for (i, value) in nodes.into_iter().enumerate() {
        let (allow_inline_multi_line, allow_inline_single_line) = (value.kind() == SyntaxKind::INLINE_TABLE, false);
        let lines_span = if compute_lines_span {
          Some(ir_helpers::LinesSpan {
            start_line: context.get_line_number_at_pos(value.start_including_leading_comments()),
            end_line: context.get_line_number_at_pos(value.text_range().end().into()),
          })
        } else {
          None
        };
        let items = ir_helpers::new_line_group({
          let generated_comma = if i == nodes_count - 1 {
            // todo: make this conditional based on config
            let is_multi_line = is_multi_line_ref.create_resolver();
            if_true("commaIfMultiLine", is_multi_line, ",".into()).into()
          } else {
            ",".into()
          };
          gen_comma_separated_value(value, generated_comma, context)
        });
        generated_nodes.push(ir_helpers::GeneratedValue {
          items,
          lines_span,
          allow_inline_multi_line,
          allow_inline_single_line,
        });
      }

      generated_nodes
    },
    ir_helpers::GenSeparatedValuesOptions {
      prefer_hanging: opts.prefer_hanging,
      force_use_new_lines: opts.force_use_new_lines,
      allow_blank_lines: opts.allow_blank_lines,
      single_line_options: SingleLineOptions {
        space_at_start: opts.single_line_space_at_start,
        space_at_end: opts.single_line_space_at_end,
        separator: opts.custom_single_line_separator.unwrap_or(Signal::SpaceOrNewLine.into()),
      },
      indent_width,
      multi_line_options: opts.multi_line_options,
      force_possible_newline_at_start: opts.force_possible_newline_at_start,
    },
  )
  .items
}

fn gen_comma_separated_value<'a>(value: SyntaxElement, generated_comma: PrintItems, context: &mut Context<'a>) -> PrintItems {
  let mut items = PrintItems::new();
  let comma_token = get_next_comma_sibling(value.clone());

  let generated_comma = generated_comma.into_rc_path();
  items.extend(gen_node_with_inner(value, context, move |mut items, _| {
    // this Rc clone is necessary because we can't move the captured generated_comma out of this closure
    items.push_optional_path(generated_comma);
    items
  }));

  // get the trailing comments after the comma token
  if let Some(comma_token) = comma_token {
    items.extend(gen_trailing_comment(comma_token.into(), context));
  }

  items
}

fn gen_trailing_comment<'a>(element: SyntaxElement, context: &mut Context<'a>) -> PrintItems {
  match get_trailing_comment(element) {
    Some(comment) => gen_comment(comment, context),
    None => PrintItems::new(),
  }
}

fn gen_comment<'a>(comment: SyntaxToken, context: &mut Context<'a>) -> PrintItems {
  let pos = comment.text_range().start().into();
  if context.has_handled_comment(pos) {
    return PrintItems::new();
  }
  context.add_handled_comment(pos);

  #[cfg(debug_assertions)]
  debug_assert_kind(comment.clone().into(), SyntaxKind::COMMENT);

  let mut items = PrintItems::new();
  items.push_condition(if_false("spaceIfNotStartOfLine", condition_resolvers::is_start_of_line(), " ".into()));
  items.extend({
    if context.config.comment_force_leading_space {
      let info = get_comment_text_info(comment.text());
      let after_hash_text = &comment.text()[info.leading_hashes_count..].trim_end();
      let mut text = "#".repeat(info.leading_hashes_count);
      if !after_hash_text.is_empty() {
        if !info.has_leading_whitespace {
          text.push(' ');
        }
        text.push_str(after_hash_text);
      }
      ir_helpers::gen_from_raw_string(&text)
    } else {
      ir_helpers::gen_from_raw_string(comment.text())
    }
  });
  items.push_signal(Signal::ExpectNewLine);
  items
}

struct CommentTextInfo {
  pub has_leading_whitespace: bool,
  pub leading_hashes_count: usize,
}

fn get_comment_text_info(text: &str) -> CommentTextInfo {
  let mut leading_hashes_count = 0;
  let mut has_leading_whitespace = false;
  for c in text.chars() {
    match c {
      '#' => leading_hashes_count += 1,
      ' ' | '\t' => {
        has_leading_whitespace = true;
        break;
      }
      _ => break,
    }
  }
  CommentTextInfo {
    leading_hashes_count,
    has_leading_whitespace,
  }
}

#[allow(dead_code)]
#[cfg(debug_assertions)]
fn print_formatted_tree(node: SyntaxNode) {
  print_node_and_children(node, 0);

  fn print_node_and_children(node: SyntaxNode, indent: usize) {
    println!("{}{:?}", " ".repeat(indent), node);
    for c in node.children_with_tokens() {
      match c {
        NodeOrToken::Node(c) => {
          print_node_and_children(c.clone(), indent + 2);
        }
        NodeOrToken::Token(t) => {
          println!("{}{:?} [TOKEN]", " ".repeat(indent + 2), t);
        }
      }
    }
  }
}

fn ensure_all_kind(nodes: impl Iterator<Item = SyntaxNode>, kind: SyntaxKind) -> Result<(), ()> {
  for node in nodes {
    ensure_kind(node.into(), kind)?;
  }
  Ok(())
}

fn ensure_kind(element: SyntaxElement, kind: SyntaxKind) -> Result<(), ()> {
  if element.kind() == kind {
    Ok(())
  } else {
    Err(())
  }
}

#[cfg(debug_assertions)]
fn debug_assert_kind(element: SyntaxElement, kind: SyntaxKind) {
  if element.kind() != kind {
    panic!("Debug Assertion: Expected kind {:?}, but was {:?}", kind, element.kind());
  }
}

fn has_following_newline(token: SyntaxToken) -> bool {
  let mut element: SyntaxElement = token.into();
  while let Some(sibling) = element.next_sibling_or_token() {
    element = sibling.clone();
    match sibling {
      NodeOrToken::Token(token) => match token.kind() {
        SyntaxKind::WHITESPACE => continue,
        SyntaxKind::NEWLINE | SyntaxKind::COMMENT => return true,
        _ => break,
      },
      NodeOrToken::Node(_) => break,
    }
  }
  false
}

fn get_next_comma_sibling(mut element: SyntaxElement) -> Option<SyntaxToken> {
  while let Some(sibling) = element.next_sibling_or_token() {
    element = sibling.clone();
    match sibling {
      NodeOrToken::Token(token) => match token.kind() {
        SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT => continue,
        SyntaxKind::COMMA => return Some(token),
        _ => break,
      },
      NodeOrToken::Node(_) => break,
    }
  }
  None
}

fn get_trailing_comment(mut element: SyntaxElement) -> Option<SyntaxToken> {
  while let Some(sibling) = element.next_sibling_or_token() {
    element = sibling.clone();
    match sibling {
      NodeOrToken::Token(token) => match token.kind() {
        SyntaxKind::WHITESPACE => continue,
        SyntaxKind::COMMENT => return Some(token),
        _ => break,
      },
      NodeOrToken::Node(_) => break,
    }
  }
  None
}

fn get_children_with_non_trivia_tokens(node: SyntaxNode) -> impl Iterator<Item = SyntaxElement> {
  node.children_with_tokens().filter_map(|c| match c {
    NodeOrToken::Token(token) => {
      if token.kind() != SyntaxKind::WHITESPACE && token.kind() != SyntaxKind::COMMENT {
        Some(token.into())
      } else {
        None
      }
    }
    NodeOrToken::Node(node) => Some(node.into()),
  })
}

fn get_child_with_kind(node: SyntaxNode, kind: SyntaxKind) -> Result<SyntaxNode, ()> {
  match node.children().find(|c| c.kind() == kind) {
    Some(node) => Ok(node),
    None => Err(()),
  }
}

fn get_token_with_kind(node: SyntaxNode, kind: SyntaxKind) -> Result<SyntaxToken, ()> {
  match node.children_with_tokens().find(|c| c.kind() == kind) {
    Some(NodeOrToken::Token(token)) => Ok(token),
    _ => Err(()),
  }
}
