use dprint_core::formatting::conditions::*;
use dprint_core::formatting::ir_helpers::SingleLineOptions;
use dprint_core::formatting::*;
use dprint_core_macros::sc;
use std::rc::Rc;

use super::Context;
use crate::ast::*;
use crate::configuration::Configuration;
use crate::configuration::IndentKind;
use crate::configuration::QuoteStyle;
use crate::configuration::TrailingCommaKind;

pub fn generate(root: &Root, config: &Configuration) -> PrintItems {
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
  let indents = indent_levels(root, context.config);
  let mut items = PrintItems::new();
  let mut previous: Option<&RootItem> = None;
  for (i, item) in root.items.iter().enumerate() {
    if let Some(previous) = previous {
      items.push_signal(Signal::NewLine);
      if item.blank_line_before() && allow_blank_line(previous, item) {
        items.push_signal(Signal::NewLine);
      }
    }
    // the indent starts after the newline that precedes the item, so the writer sees an empty
    // blank line rather than one padded out with the indentation of what follows it
    items.extend(ir_helpers::with_indent_times(gen_root_item(item, context), indents[i]));
    previous = Some(item);
  }
  items
}

/// The indent level each root item is written at.
///
/// A table header's level is how many of the headers above it name one of its ancestors, so
/// `[a.b]` beneath `[a]` sits one level in while `[a.b]` beneath nothing at all sits at the
/// margin. A table's body -- its entries and the comments among them -- follows its header, one
/// level further in when entries are indented.
fn indent_levels(root: &Root, config: &Configuration) -> Vec<u32> {
  let mut levels = vec![0u32; root.items.len()];
  // the level a comment falls back to when it closes off its section rather than introducing the
  // item beneath it
  let mut section_levels = vec![0u32; root.items.len()];
  // the headers enclosing the item being looked at, each naming an ancestor of the next
  let mut open_tables: Vec<&Key> = Vec::new();
  // the whole body of a section shares one level, so it is worked out at the header rather than
  // an item at a time -- a comment sitting above the section's first entry needs it too
  let mut body_level = 0;

  for (i, item) in root.items.iter().enumerate() {
    match item {
      RootItem::TableHeader(header) => {
        while open_tables.last().is_some_and(|key| !key.is_strict_prefix_of(&header.key)) {
          open_tables.pop();
        }
        let depth = open_tables.len() as u32;
        open_tables.push(&header.key);
        let table_level = match config.indent_tables {
          IndentKind::Always => depth,
          IndentKind::Never => 0,
          IndentKind::Maintain => {
            if header.indent_in_source > 0 {
              depth
            } else {
              0
            }
          }
        };
        body_level = table_level + section_body_extra_indent(&root.items[i + 1..], header, config);
        levels[i] = table_level;
      }
      // an entry above the first table header belongs to no table, so there is nothing for it to
      // be indented beneath, and `body_level` is still zero
      RootItem::Entry(_) => levels[i] = body_level,
      RootItem::Comment(_) => {}
    }
    section_levels[i] = body_level;
  }

  // A comment is written at the indent of the item it introduces, which is only known once that
  // item has been reached, so the comments are filled in walking back through the file.
  let mut following: Option<(bool, u32)> = None;
  for i in (0..root.items.len()).rev() {
    if matches!(root.items[i], RootItem::Comment(_)) {
      levels[i] = match following {
        Some((false, level)) => level,
        // a blank line beneath it, or the end of the file, means it belongs to what came before
        _ => section_levels[i],
      };
    }
    following = Some((root.items[i].blank_line_before(), levels[i]));
  }

  levels
}

/// How many levels in from its header a section's body is written, where `rest` is everything
/// after the header.
///
/// Under `maintain` the section's first entry decides for the whole body, rather than each entry
/// deciding for itself: a table is indented or it isn't, and a comment written above that first
/// entry has to be given the same answer before the entry has been reached. A section holding
/// nothing but comments has only those to go on.
fn section_body_extra_indent(rest: &[RootItem], header: &TableHeader, config: &Configuration) -> u32 {
  match config.indent_entries {
    IndentKind::Never => 0,
    IndentKind::Always => 1,
    IndentKind::Maintain => {
      let section = rest.iter().take_while(|item| !item.is_table_header());
      let indent = section
        .clone()
        .find_map(|item| match item {
          RootItem::Entry(entry) => Some(entry.indent_in_source),
          _ => None,
        })
        .or_else(|| {
          section.into_iter().find_map(|item| match item {
            RootItem::Comment(comment) => Some(comment.indent_in_source),
            _ => None,
          })
        });
      u32::from(indent.is_some_and(|indent| indent > header.indent_in_source))
    }
  }
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
  items.push_sc(if context.config.space_surrounding_equals { sc!(" = ") } else { sc!("=") });
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
    ValueKind::Scalar(text) => match requoted_string(text, context.config.quote_style) {
      Some(text) => ir_helpers::gen_from_string(&text),
      None => ir_helpers::gen_from_string(text),
    },
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

/// The text of a single-line string rewritten with the preferred quote, or `None` when it is
/// already written with it or cannot be rewritten.
///
/// A basic string reads a backslash as beginning an escape where a literal string reads it as
/// itself, so the two spell the same value differently as soon as one appears. Rewriting such a
/// string means rewriting its contents, and rewriting one that already holds the preferred quote
/// means escaping that quote — neither of which is done here. The point is to settle on one quote
/// wherever it costs nothing, not to make every string look the same.
fn requoted_string(text: &str, style: QuoteStyle) -> Option<String> {
  let (from, to) = match style {
    QuoteStyle::Maintain => return None,
    QuoteStyle::PreferDouble => ('\'', '"'),
    QuoteStyle::PreferSingle => ('"', '\''),
  };
  let inner = text.strip_prefix(from)?.strip_suffix(from)?;
  if inner.contains('\\') || inner.contains(to) {
    return None;
  }
  let mut result = String::with_capacity(text.len());
  result.push(to);
  result.push_str(inner);
  result.push(to);
  Some(result)
}

// ---- arrays ----

fn gen_array(array: &Array, context: &mut Context) -> PrintItems {
  let force_use_new_lines = array.force_use_new_lines(context.config);
  let space_within_single_line = context.config.array_space_surrounding_brackets;
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
      gen_separated(&array.values, force_use_new_lines, space_within_single_line, context)
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
  if !context.is_in_single_line_table() && table.force_use_new_lines(context.config) {
    return gen_multi_line_inline_table(table, context);
  }

  let pad = context.config.inline_table_space_surrounding_braces && !table.entries.is_empty();
  let mut items = PrintItems::new();
  context.with_single_line_table(|context| {
    items.push_sc(sc!("{"));
    for (i, entry) in table.entries.iter().enumerate() {
      if i > 0 {
        items.push_sc(sc!(", "));
      } else if pad {
        items.push_sc(sc!(" "));
      }
      items.extend(gen_entry_without_trailing_comment(entry, context));
    }
    items.push_sc(if pad { sc!(" }") } else { sc!("}") });
  });

  // Spec:
  // > Inline tables are intended to appear on a single line. A terminating comma (also called trailing comma)
  // > is not permitted after the last key/value pair in an inline table. No newlines are allowed between the
  // > curly braces unless they are valid within a value. Even so, it is strongly discouraged to break an inline
  // > table onto multiples lines. If you find yourself gripped with this desire, it means you should be using
  // > standard tables.
  //
  // Note the "unless they are valid within a value" carve out: an array between the braces may be
  // written over several lines, and so may a multi-line string, because those newlines belong to
  // the value rather than to the table. Nothing at the table's own level can break -- its braces,
  // commas and equals signs are all hard text, and a table nested within it is kept on one line --
  // so the table stays on its line whatever a value inside it does.
  items
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
      gen_separated(&table.entries, true, false, context)
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
  /// Whether the item is written over more than one line however it is formatted.
  is_known_multi_line: bool,
  entry: SeparatedItemValue<'a>,
}

enum SeparatedItemValue<'a> {
  Value(&'a Value<'a>),
  Entry(&'a Entry<'a>),
}

/// Whether a value is written over several lines depends on the configuration, so this stands in
/// for the `From` conversion the item would otherwise be built by.
trait IntoSeparatedItem<'a> {
  fn into_separated_item(self, config: &Configuration, within_single_line_table: bool) -> SeparatedItem<'a>;
}

impl<'a> IntoSeparatedItem<'a> for &'a ArrayValue<'a> {
  fn into_separated_item(self, config: &Configuration, within_single_line_table: bool) -> SeparatedItem<'a> {
    SeparatedItem {
      leading_comments: &self.leading_comments,
      blank_line_before: self.blank_line_before,
      trailing_comment: self.trailing_comment.as_ref(),
      blank_line_before_item: blank_line_before_item(&self.leading_comments, self.blank_line_before),
      is_known_multi_line: self.value.is_known_multi_line(config, within_single_line_table),
      entry: SeparatedItemValue::Value(&self.value),
    }
  }
}

impl<'a> IntoSeparatedItem<'a> for &'a Entry<'a> {
  fn into_separated_item(self, config: &Configuration, within_single_line_table: bool) -> SeparatedItem<'a> {
    SeparatedItem {
      leading_comments: &self.leading_comments,
      blank_line_before: self.blank_line_before,
      trailing_comment: self.trailing_comment.as_ref(),
      blank_line_before_item: blank_line_before_item(&self.leading_comments, self.blank_line_before),
      is_known_multi_line: self.value.is_known_multi_line(config, within_single_line_table),
      entry: SeparatedItemValue::Entry(self),
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
fn gen_separated<'a, T>(items: &'a [T], force_use_new_lines: bool, space_within_single_line: bool, context: &mut Context) -> PrintItems
where
  &'a T: IntoSeparatedItem<'a>,
{
  let indent_width = context.config.indent_width;
  let trailing_commas = context.config.trailing_commas;
  let within_single_line_table = context.is_in_single_line_table();
  ir_helpers::gen_separated_values(
    |is_multi_line_ref| {
      let count = items.len();
      let mut generated = Vec::with_capacity(count);
      // Synthetic line numbers rather than source positions: a blank line is preserved because the
      // author asked for one, and the item may since have been moved by the Cargo.toml sorting,
      // which would leave any position taken from the source pointing at the wrong line.
      let mut line = 0;
      for (i, item) in items
        .iter()
        .map(|item| item.into_separated_item(context.config, within_single_line_table))
        .enumerate()
      {
        if i > 0 {
          line += if item.blank_line_before_item { 2 } else { 1 };
        }
        let lines_span = Some(ir_helpers::LinesSpan {
          start_line: line,
          end_line: line,
        });
        let generated_comma = if i == count - 1 {
          match trailing_commas {
            TrailingCommaKind::Never => PrintItems::new(),
            TrailingCommaKind::OnlyMultiLine => {
              let is_multi_line = is_multi_line_ref.create_resolver();
              if_true("commaIfMultiLine", is_multi_line, ",".into()).into()
            }
          }
        } else {
          ",".into()
        };
        let is_known_multi_line = item.is_known_multi_line;
        generated.push(ir_helpers::GeneratedValue {
          items: ir_helpers::new_line_group(gen_separated_item(item, generated_comma, context)),
          lines_span,
          // a value spanning several lines always breaks its group up, wherever it sits in it
          allow_inline_multi_line: false,
          allow_inline_single_line: false,
          is_known_multi_line,
        });
      }
      generated
    },
    ir_helpers::GenSeparatedValuesOptions {
      prefer_hanging: false,
      force_use_new_lines,
      allow_blank_lines: true,
      single_line_options: SingleLineOptions {
        space_at_start: space_within_single_line,
        space_at_end: space_within_single_line,
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
