use dprint_core::formatting::conditions::*;
use dprint_core::formatting::ir_helpers::SingleLineOptions;
use dprint_core::formatting::*;
use dprint_core_macros::sc;
use std::rc::Rc;

use super::Context;
use crate::ast::*;

pub fn generate(root: &Root, config: &crate::configuration::Configuration) -> PrintItems {
  let mut context = Context::new(config);
  let mut items = gen_root(root, &mut context);
  items.push_condition(if_true(
    "endOfFileNewLine",
    Rc::new(|context| Some(context.writer_info.column_number > 0 || context.writer_info.line_number > 0)),
    Signal::NewLine.into(),
  ));
  items
}

fn gen_root(root: &Root, context: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();
  let mut previous: Option<&RootItem> = None;
  for item in &root.items {
    if let Some(previous) = previous {
      items.push_signal(Signal::NewLine);
      if item.blank_line_before() && allow_blank_line(previous, item) {
        items.push_signal(Signal::NewLine);
      }
    }
    items.extend(gen_root_item(item, context));
    previous = Some(item);
  }
  items
}

/// A blank line is kept everywhere except directly beneath a table header, where it only separates
/// one header from another.
fn allow_blank_line(previous: &RootItem, current: &RootItem) -> bool {
  current.is_table_header() || !previous.is_table_header()
}

fn gen_root_item(item: &RootItem, context: &mut Context) -> PrintItems {
  match item {
    RootItem::Comment(comment) => gen_comment(comment, context),
    RootItem::Entry(entry) => gen_entry(entry, context),
    RootItem::TableHeader(header) => gen_table_header(header, context),
  }
}

fn gen_table_header(header: &TableHeader, context: &mut Context) -> PrintItems {
  // Spec: Naming rules for tables are the same as for keys
  let mut items = PrintItems::new();
  items.push_sc(if header.is_array_of_tables { sc!("[[") } else { sc!("[") });
  items.extend(gen_key(&header.key));
  items.push_sc(if header.is_array_of_tables { sc!("]]") } else { sc!("]") });
  if let Some(comment) = &header.trailing_comment {
    items.extend(gen_comment(comment, context));
  }
  items
}

fn gen_entry(entry: &Entry, context: &mut Context) -> PrintItems {
  let mut items = gen_entry_without_trailing_comment(entry, context);
  if let Some(comment) = &entry.trailing_comment {
    items.extend(gen_comment(comment, context));
  }
  items
}

fn gen_entry_without_trailing_comment(entry: &Entry, context: &mut Context) -> PrintItems {
  let mut items = gen_key(&entry.key);
  items.push_sc(sc!(" = "));
  items.extend(gen_value(&entry.value, context));
  items
}

/// Spec: A key may be either bare, quoted, or dotted.
fn gen_key(key: &Key) -> PrintItems {
  let mut items = PrintItems::new();
  for (i, part) in key.parts().enumerate() {
    if i > 0 {
      items.push_sc(sc!("."));
    }
    items.extend(ir_helpers::gen_from_string(part.text));
  }
  items
}

/// Spec: Values must be either String, Integer, Float, Boolean, DateTimes, Array, InlineTable
fn gen_value(value: &Value, context: &mut Context) -> PrintItems {
  match &value.kind {
    ValueKind::Scalar(text) => ir_helpers::gen_from_string(text),
    ValueKind::MultiLineString(text) => {
      let mut items = PrintItems::new();
      items.push_force_current_line_indentation();
      items.extend(ir_helpers::gen_from_raw_string(text));
      items
    }
    ValueKind::Array(array) => gen_array(array, context),
    ValueKind::InlineTable(table) => gen_inline_table(table, context),
  }
}

// ---- arrays ----

fn gen_array(array: &Array, context: &mut Context) -> PrintItems {
  if context.is_in_single_line_table() {
    // An array within a table that has to stay on one line is collapsed onto that line, so its
    // values are separated by hard spaces rather than by anything that could break. Its comments
    // are still written: a comment ends its own line whatever we do, and those newlines sit within
    // a value, which TOML 1.0 allows between the braces.
    return gen_surrounded(
      SurroundedParams {
        open: sc!("["),
        close: sc!("]"),
        comment_after_open: array.comment_after_open.as_ref(),
        comments_before_close: &array.comments_before_close,
      },
      |context| {
        let mut items = PrintItems::new();
        for (i, value) in array.values.iter().enumerate() {
          // space away from the previous value, unless its comment already ended that line
          if i > 0 && array.values[i - 1].trailing_comment.is_none() {
            items.push_sc(sc!(" "));
          }
          for comment in &value.leading_comments {
            items.extend(gen_comment(comment, context));
            items.push_signal(Signal::NewLine);
          }
          items.extend(gen_value(&value.value, context));
          // the comma goes before the comment rather than after it, so that a comment ending its
          // line doesn't leave the next value's comma leading the line beneath
          if i + 1 < array.values.len() {
            items.push_sc(sc!(","));
          }
          if let Some(comment) = &value.trailing_comment {
            items.extend(gen_comment(comment, context));
          }
        }
        items
      },
      context,
    );
  }

  let force_use_new_lines = array.force_use_new_lines();
  gen_surrounded(
    SurroundedParams {
      open: sc!("["),
      close: sc!("]"),
      comment_after_open: array.comment_after_open.as_ref(),
      comments_before_close: &array.comments_before_close,
    },
    |context| {
      if array.values.is_empty() {
        return if force_use_new_lines { Signal::NewLine.into() } else { PrintItems::new() };
      }
      gen_separated(&array.values, force_use_new_lines, context)
    },
    context,
  )
}

// ---- inline tables ----

fn gen_inline_table(table: &InlineTable, context: &mut Context) -> PrintItems {
  // TOML 1.1 allows an inline table to be written over several lines. The author chose that, so
  // keep it; a table written on one line is never expanded, which would produce syntax a 1.0 parser
  // rejects. A table holding a comment of its own is multi-line whatever follows its brace, since a
  // comment runs to the end of its line, and generating it on one line would drop the comment. A
  // comment inside one of its values is not the table's, and expanding the table over it would turn
  // a 1.0 document into a 1.1 one.
  //
  // Nothing within a table that has to stay on one line may break, so a table nested in one is
  // generated on a single line however the author wrote it.
  if !context.is_in_single_line_table() && (table.multi_line_in_source || table.has_own_comment()) {
    return gen_multi_line_inline_table(table, context);
  }

  let mut items = PrintItems::new();
  context.with_single_line_table(|context| {
    items.push_sc(sc!("{"));
    for (i, entry) in table.entries.iter().enumerate() {
      items.push_sc(if i > 0 { sc!(", ") } else { sc!(" ") });
      items.extend(gen_entry_without_trailing_comment(entry, context));
    }
    items.push_sc(if table.entries.is_empty() { sc!("}") } else { sc!(" }") });
  });

  // Spec:
  // > Inline tables are intended to appear on a single line. A terminating comma (also called trailing comma)
  // > is not permitted after the last key/value pair in an inline table. No newlines are allowed between the
  // > curly braces unless they are valid within a value. Even so, it is strongly discouraged to break an inline
  // > table onto multiples lines. If you find yourself gripped with this desire, it means you should be using
  // > standard tables.
  //
  // Note the "unless they are valid within a value" carve out. The newlines of a multi-line string
  // are part of its value, and the printer discards every newline signal within a force-no-new-lines
  // scope, so a table holding one has to be generated without the scope. Everything within is
  // separated by hard spaces, so dropping it doesn't let anything wrap.
  if table.entries.iter().any(|entry| entry.value.contains_multi_line_string()) {
    items
  } else {
    ir_helpers::with_no_new_lines(items)
  }
}

fn gen_multi_line_inline_table(table: &InlineTable, context: &mut Context) -> PrintItems {
  gen_surrounded(
    SurroundedParams {
      open: sc!("{"),
      close: sc!("}"),
      comment_after_open: table.comment_after_open.as_ref(),
      comments_before_close: &table.comments_before_close,
    },
    |context| {
      if table.entries.is_empty() {
        return Signal::NewLine.into();
      }
      gen_separated(&table.entries, true, context)
    },
    context,
  )
}

// ---- shared bracket handling ----

struct SurroundedParams<'a> {
  open: &'static StringContainer,
  close: &'static StringContainer,
  comment_after_open: Option<&'a Comment<'a>>,
  comments_before_close: &'a [Comment<'a>],
}

fn gen_surrounded(params: SurroundedParams, gen_inner: impl FnOnce(&mut Context) -> PrintItems, context: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();
  items.push_sc(params.open);
  if let Some(comment) = params.comment_after_open {
    items.extend(gen_comment(comment, context));
  }

  items.extend(gen_inner(context));

  for comment in params.comments_before_close {
    if comment.blank_line_before {
      items.push_signal(Signal::NewLine);
    }
    items.extend(ir_helpers::with_indent(gen_comment(comment, context)));
    items.push_signal(Signal::NewLine);
  }

  items.push_sc(params.close);
  items
}

/// One comma separated item of an array or a multi-line inline table.
struct SeparatedItem<'a> {
  leading_comments: &'a [Comment<'a>],
  /// Whether a blank line sits between the last leading comment and the item itself.
  blank_line_before: bool,
  trailing_comment: Option<&'a Comment<'a>>,
  /// Whether a blank line separates this whole item, comments included, from the one before it.
  blank_line_before_item: bool,
  entry: SeparatedItemValue<'a>,
}

enum SeparatedItemValue<'a> {
  Value(&'a Value<'a>),
  Entry(&'a Entry<'a>),
}

impl<'a> From<&'a ArrayValue<'a>> for SeparatedItem<'a> {
  fn from(value: &'a ArrayValue<'a>) -> Self {
    SeparatedItem {
      leading_comments: &value.leading_comments,
      blank_line_before: value.blank_line_before,
      trailing_comment: value.trailing_comment.as_ref(),
      blank_line_before_item: blank_line_before_item(&value.leading_comments, value.blank_line_before),
      entry: SeparatedItemValue::Value(&value.value),
    }
  }
}

impl<'a> From<&'a Entry<'a>> for SeparatedItem<'a> {
  fn from(entry: &'a Entry<'a>) -> Self {
    SeparatedItem {
      leading_comments: &entry.leading_comments,
      blank_line_before: entry.blank_line_before,
      trailing_comment: entry.trailing_comment.as_ref(),
      blank_line_before_item: blank_line_before_item(&entry.leading_comments, entry.blank_line_before),
      entry: SeparatedItemValue::Entry(entry),
    }
  }
}

/// A blank line above an item sits above its comments when it has any.
fn blank_line_before_item(leading_comments: &[Comment<'_>], blank_line_before: bool) -> bool {
  match leading_comments.first() {
    Some(comment) => comment.blank_line_before,
    None => blank_line_before,
  }
}

/// `items` is the array's values or the table's entries; each is turned into a [`SeparatedItem`]
/// as it is reached rather than up front, so the whole run is never collected into a `Vec`.
fn gen_separated<'a, T>(items: &'a [T], force_use_new_lines: bool, context: &mut Context) -> PrintItems
where
  &'a T: Into<SeparatedItem<'a>>,
{
  let indent_width = context.config.indent_width;
  ir_helpers::gen_separated_values(
    |is_multi_line_ref| {
      let count = items.len();
      let mut generated = Vec::with_capacity(count);
      // Synthetic line numbers rather than source positions: a blank line is preserved because the
      // author asked for one, and the item may since have been moved by the Cargo.toml sorting,
      // which would leave any position taken from the source pointing at the wrong line.
      let mut line = 0;
      for (i, item) in items.iter().map(Into::into).enumerate() {
        if i > 0 {
          line += if item.blank_line_before_item { 2 } else { 1 };
        }
        let lines_span = Some(ir_helpers::LinesSpan {
          start_line: line,
          end_line: line,
        });
        let generated_comma = if i == count - 1 {
          // todo: make this conditional based on config
          let is_multi_line = is_multi_line_ref.create_resolver();
          if_true("commaIfMultiLine", is_multi_line, ",".into()).into()
        } else {
          ",".into()
        };
        generated.push(ir_helpers::GeneratedValue {
          items: ir_helpers::new_line_group(gen_separated_item(item, generated_comma, context)),
          lines_span,
          // a value spanning several lines always breaks its group up, wherever it sits in it
          allow_inline_multi_line: false,
          allow_inline_single_line: false,
        });
      }
      generated
    },
    ir_helpers::GenSeparatedValuesOptions {
      prefer_hanging: false,
      force_use_new_lines,
      allow_blank_lines: true,
      single_line_options: SingleLineOptions {
        space_at_start: false,
        space_at_end: false,
        separator: Signal::SpaceOrNewLine.into(),
      },
      indent_width,
      multi_line_options: ir_helpers::MultiLineOptions::surround_newlines_indented(),
      force_possible_newline_at_start: false,
    },
  )
  .items
}

fn gen_separated_item(item: SeparatedItem, generated_comma: PrintItems, context: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();
  for (i, comment) in item.leading_comments.iter().enumerate() {
    // a blank above the first comment separates this item from the previous one, which the
    // separator between items has already accounted for
    if i > 0 && comment.blank_line_before {
      items.push_signal(Signal::NewLine);
    }
    items.extend(gen_comment(comment, context));
    items.push_signal(Signal::NewLine);
  }
  if item.blank_line_before && !item.leading_comments.is_empty() {
    items.push_signal(Signal::NewLine);
  }

  items.extend(match item.entry {
    SeparatedItemValue::Value(value) => gen_value(value, context),
    SeparatedItemValue::Entry(entry) => gen_entry_without_trailing_comment(entry, context),
  });
  items.extend(generated_comma);

  if let Some(comment) = item.trailing_comment {
    items.extend(gen_comment(comment, context));
  }
  items
}

// ---- comments ----

fn gen_comment(comment: &Comment, context: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();
  items.push_condition(if_false("spaceIfNotStartOfLine", condition_resolvers::is_start_of_line(), " ".into()));
  items.extend(gen_comment_text(comment, context));
  items.push_signal(Signal::ExpectNewLine);
  items
}

fn gen_comment_text(comment: &Comment, context: &mut Context) -> PrintItems {
  if !context.config.comment_force_leading_space {
    return ir_helpers::gen_from_raw_string(comment.text);
  }

  let info = get_comment_text_info(comment.text);
  let after_hash_text = comment.text[info.start_text_index..].trim_end_matches([' ', '\t']);
  // Nothing is inserted when the text after the hashes already begins with whitespace, or when
  // there is no text at all, so rebuilding would only reproduce the source text. Almost every
  // comment in a real file takes this path, and rendering the slice saves building the copy.
  if info.has_leading_whitespace || after_hash_text.is_empty() {
    return ir_helpers::gen_from_raw_string(&comment.text[..info.start_text_index + after_hash_text.len()]);
  }

  let mut text = String::with_capacity(comment.text.len() + 1);
  for _ in 0..info.leading_hashes_count {
    text.push('#');
  }
  if info.has_exclamation_point {
    text.push('!');
  }
  text.push(' ');
  text.push_str(after_hash_text);
  ir_helpers::gen_from_raw_string(&text)
}

struct CommentTextInfo {
  pub has_leading_whitespace: bool,
  pub has_exclamation_point: bool,
  pub leading_hashes_count: usize,
  pub start_text_index: usize,
}

fn get_comment_text_info(text: &str) -> CommentTextInfo {
  let mut leading_hashes_count = 0;
  let mut has_leading_whitespace = false;
  let mut has_exclamation_point = false;
  let mut start_text_index = 0;
  let mut chars = text.char_indices();
  for (index, c) in chars.by_ref() {
    match c {
      '#' if !has_exclamation_point => {
        leading_hashes_count += 1;
        start_text_index = index + 1;
      }
      '!' if leading_hashes_count == 1 => {
        has_exclamation_point = true;
        start_text_index = index + 1;
        if matches!(chars.next(), Some((_, ' ' | '\t'))) {
          has_leading_whitespace = true;
        }
        break;
      }
      ' ' | '\t' => {
        has_leading_whitespace = true;
        break;
      }
      _ => break,
    }
  }
  CommentTextInfo {
    leading_hashes_count,
    has_exclamation_point,
    has_leading_whitespace,
    start_text_index,
  }
}
