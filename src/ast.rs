// AST types for the TOML parser.
//
// The tree is lossless for everything the formatter cares about: values keep their source text
// verbatim, and comments and blank lines are attached to the construct they belong to rather than
// being recovered by walking siblings. Anything not represented here is insignificant whitespace.
//
// Text is borrowed from the source. Every piece of text the tree holds is a slice of the input,
// which outlives the tree, so none of it is copied. Nothing rewrites a node's text -- the Cargo
// conventions reorder nodes and adjust their blank-line flags, but the text itself is only ever
// read -- so a plain `&str` is enough and a `Cow` would only make each node larger.

use crate::configuration::Configuration;

/// A comment, from the `#` up to (but not including) the end of the line.
#[derive(Debug, Clone)]
pub struct Comment<'a> {
  /// The comment's source text, including the leading `#`.
  pub text: &'a str,
  /// Whether a blank line separates this comment from whatever precedes it.
  pub blank_line_before: bool,
  /// How many whitespace characters preceded the comment on its line, which is what the
  /// `maintain` indent settings go by. Zero for a comment that isn't on a line of its own at the
  /// top level, which is never indented independently.
  pub indent_in_source: usize,
}

/// A parsed TOML document.
#[derive(Debug, Clone)]
pub struct Root<'a> {
  pub items: Vec<RootItem<'a>>,
}

/// A top level item. Comments on their own line are items in their own right rather than trivia
/// attached to the following item, which is what lets blank lines around them be preserved.
#[derive(Debug, Clone)]
pub enum RootItem<'a> {
  Comment(Comment<'a>),
  Entry(Entry<'a>),
  TableHeader(TableHeader<'a>),
}

impl RootItem<'_> {
  pub fn blank_line_before(&self) -> bool {
    match self {
      RootItem::Comment(c) => c.blank_line_before,
      RootItem::Entry(e) => e.blank_line_before,
      RootItem::TableHeader(h) => h.blank_line_before,
    }
  }

  pub fn is_table_header(&self) -> bool {
    matches!(self, RootItem::TableHeader(_))
  }
}

/// A table header, either `[key]` or `[[key]]`.
#[derive(Debug, Clone)]
pub struct TableHeader<'a> {
  pub key: Key<'a>,
  /// Whether this is an array of tables header (`[[key]]`).
  pub is_array_of_tables: bool,
  pub blank_line_before: bool,
  pub trailing_comment: Option<Comment<'a>>,
  /// How many whitespace characters preceded the header on its line, which is what the
  /// `maintain` indent settings go by.
  pub indent_in_source: usize,
}

/// A key/value pair.
#[derive(Debug, Clone)]
pub struct Entry<'a> {
  pub key: Key<'a>,
  pub value: Value<'a>,
  pub blank_line_before: bool,
  pub trailing_comment: Option<Comment<'a>>,
  /// Comments on the lines immediately above this entry. Only populated for entries inside an
  /// inline table; at the top level such comments are separate [`RootItem::Comment`]s.
  pub leading_comments: Vec<Comment<'a>>,
  /// How many whitespace characters preceded the entry on its line, which is what the `maintain`
  /// indent settings go by. Always zero for an entry inside an inline table, which is never
  /// indented on its own.
  pub indent_in_source: usize,
}

/// A key, which may be dotted (`a.b.c`).
///
/// The first segment is held inline because a dotted key is the exception: keeping the rest in a
/// `Vec` means the overwhelmingly common single segment key allocates nothing at all.
#[derive(Debug, Clone)]
pub struct Key<'a> {
  pub first: KeyPart<'a>,
  pub rest: Vec<KeyPart<'a>>,
}

impl<'a> Key<'a> {
  /// The key's dot separated segments, of which there is always at least one.
  pub fn parts(&self) -> impl Iterator<Item = &KeyPart<'a>> {
    std::iter::once(&self.first).chain(self.rest.iter())
  }
}

impl Key<'_> {
  /// Whether the key names `text`, a dotted name whose segments are unquoted. Comparing rather
  /// than building the name keeps this from allocating, which matters because the Cargo
  /// conventions ask it of every table header in the file.
  ///
  /// `["dependencies"]` names the same table as `[dependencies]`, so the quotes around a quoted
  /// segment are ignored. Any escape within a basic string is left as written, which is enough to
  /// compare against a plain name like `package`.
  pub fn names(&self, text: &str) -> bool {
    let mut rest = text;
    for (i, part) in self.parts().enumerate() {
      if i > 0 {
        match rest.strip_prefix('.') {
          Some(remaining) => rest = remaining,
          None => return false,
        }
      }
      match rest.strip_prefix(part.unquoted_text()) {
        Some(remaining) => rest = remaining,
        None => return false,
      }
    }
    rest.is_empty()
  }

  /// Whether this key names a strict ancestor of `other`, as `[a]` does of `[a.b]`. Quoting is
  /// ignored, since `[a."b"]` names the same table as `[a.b]`.
  pub fn is_strict_prefix_of(&self, other: &Key<'_>) -> bool {
    let mut other_parts = other.parts();
    for part in self.parts() {
      match other_parts.next() {
        Some(other_part) if other_part.unquoted_text() == part.unquoted_text() => {}
        _ => return false,
      }
    }
    other_parts.next().is_some()
  }
}

/// One dot separated segment of a key. Bare, quoted and literal keys are all kept verbatim, since
/// the formatter reproduces the segment as written.
#[derive(Debug, Clone)]
pub struct KeyPart<'a> {
  pub text: &'a str,
}

impl KeyPart<'_> {
  /// The segment with its surrounding quotes removed, naming the key rather than spelling it. Any
  /// escape within a basic string is left as written, which is enough to compare against a plain
  /// name like `package`.
  pub fn unquoted_text(&self) -> &str {
    for quote in ['"', '\''] {
      if let Some(inner) = self.text.strip_prefix(quote).and_then(|t| t.strip_suffix(quote)) {
        return inner;
      }
    }
    self.text
  }
}

/// A value.
#[derive(Debug, Clone)]
pub struct Value<'a> {
  pub kind: ValueKind<'a>,
}

impl Value<'_> {
  /// Whether this value is written over more than one line however it is formatted.
  ///
  /// Telling a group this in advance saves it printing its values on the assumption that they fit
  /// on one line only to find out otherwise, which for values nested inside one another it would
  /// otherwise repeat at every level. It has to be certain, so an inline table the author wrote on
  /// one line counts for nothing within it except a string, whose own newlines are kept wherever it
  /// appears.
  /// `within_single_line_table` is whether an enclosing inline table is written on a single line,
  /// which keeps any table within it -- including one reached through an array -- on that line too.
  pub fn is_known_multi_line(&self, config: &Configuration, within_single_line_table: bool) -> bool {
    match &self.kind {
      // a triple quoted string is only written over several lines if its contents are
      ValueKind::MultiLineString(text) => text.contains('\n'),
      ValueKind::Scalar(_) => false,
      // an array may be broken up wherever it sits, since its newlines are within a value
      ValueKind::Array(array) => {
        array.force_use_new_lines(config)
          || array
            .values
            .iter()
            .any(|value| value.value.is_known_multi_line(config, within_single_line_table))
      }
      ValueKind::InlineTable(table) => {
        let broken_up = !within_single_line_table && table.force_use_new_lines(config);
        broken_up || table.entries.iter().any(|entry| entry.value.is_known_multi_line(config, !broken_up))
      }
    }
  }

  /// Whether an inline table anywhere within this value holds a comment of its own.
  ///
  /// Such a comment is only written when its table is written over several lines, so a value
  /// containing one can never be collapsed onto a single line without losing it. An array's own
  /// comments don't count: an array keeps them however the table around it is written, since the
  /// newlines they bring sit within a value, which TOML 1.0 already allows between braces.
  fn contains_inline_table_comment(&self) -> bool {
    match &self.kind {
      ValueKind::Scalar(_) | ValueKind::MultiLineString(_) => false,
      ValueKind::Array(array) => array.values.iter().any(|value| value.value.contains_inline_table_comment()),
      ValueKind::InlineTable(table) => table.contains_own_comment(),
    }
  }
}

#[derive(Debug, Clone)]
pub enum ValueKind<'a> {
  /// A single-line value kept verbatim: a string, number, boolean or date-time.
  Scalar(&'a str),
  /// A multi-line basic or literal string, kept verbatim including its newlines.
  MultiLineString(&'a str),
  Array(Array<'a>),
  InlineTable(InlineTable<'a>),
}

/// An array (`[1, 2, 3]`).
#[derive(Debug, Clone)]
pub struct Array<'a> {
  pub values: Vec<ArrayValue<'a>>,
  /// A comment on the same line as the opening bracket (`[ # here`).
  pub comment_after_open: Option<Comment<'a>>,
  /// Comments on their own lines between the last value and the closing bracket.
  pub comments_before_close: Vec<Comment<'a>>,
  /// Whether the opening bracket is followed by a newline or a comment, meaning the author wrote
  /// the array over multiple lines.
  pub multi_line_in_source: bool,
}

impl Array<'_> {
  /// Whether the array should be printed over multiple lines. An array the author broke up is kept
  /// broken up, but one that holds nothing at all collapses.
  pub fn force_use_new_lines(&self, config: &Configuration) -> bool {
    // A comment written on its own line before the closing bracket is only given a line of its own
    // when the array is broken up. Left on one line it is printed against the last value, turning
    // it into that value's trailing comment and formatting differently the second time around.
    if !self.comments_before_close.is_empty() {
      return true;
    }
    if config.array_prefer_single_line {
      // The author's layout no longer decides this, so only a comment does. Any comment runs to the
      // end of its line, so an array holding one can't be written on a single line.
      return self.has_own_comment();
    }
    self.multi_line_in_source && !(self.values.is_empty() && self.comment_after_open.is_none())
  }

  /// Whether a comment sits directly within this array's brackets rather than inside one of its
  /// values.
  fn has_own_comment(&self) -> bool {
    self.comment_after_open.is_some()
      || !self.comments_before_close.is_empty()
      || self
        .values
        .iter()
        .any(|value| value.trailing_comment.is_some() || !value.leading_comments.is_empty())
  }
}

/// A value within an array, along with the comments attached to it.
#[derive(Debug, Clone)]
pub struct ArrayValue<'a> {
  pub value: Value<'a>,
  /// Comments on the lines immediately above the value.
  pub leading_comments: Vec<Comment<'a>>,
  /// A comment on the same line as the value, before or after its comma.
  pub trailing_comment: Option<Comment<'a>>,
  pub blank_line_before: bool,
}

/// An inline table (`{ a = 1, b = 2 }`).
#[derive(Debug, Clone)]
pub struct InlineTable<'a> {
  pub entries: Vec<Entry<'a>>,
  /// A comment on the same line as the opening brace.
  pub comment_after_open: Option<Comment<'a>>,
  /// Comments on their own lines between the last entry and the closing brace.
  pub comments_before_close: Vec<Comment<'a>>,
  /// Whether the author wrote the table over multiple lines, which TOML 1.1 permits.
  pub multi_line_in_source: bool,
}

impl InlineTable<'_> {
  /// Whether a comment sits directly within this table's braces rather than inside one of its
  /// values.
  ///
  /// Only such a comment forces the table itself onto several lines. A comment inside a value —
  /// within a nested array, say — sits among that value's own lines, which TOML 1.0 already permits
  /// between the braces: "No newlines are allowed between the curly braces unless they are valid
  /// within a value."
  pub fn has_own_comment(&self) -> bool {
    self.comment_after_open.is_some()
      || !self.comments_before_close.is_empty()
      || self
        .entries
        .iter()
        .any(|entry| entry.trailing_comment.is_some() || !entry.leading_comments.is_empty())
  }

  /// Whether this table, or one nested within it, holds a comment of its own.
  ///
  /// A table nested inside one written on a single line is written on a single line too, and a
  /// single line has nowhere to put a comment, so the table around it has to be broken up as well.
  /// Doing so is always safe: a comment between braces runs to the end of its line, so a table
  /// holding one was already written over several lines and the document is already TOML 1.1.
  pub fn contains_own_comment(&self) -> bool {
    self.has_own_comment() || self.entries.iter().any(|entry| entry.value.contains_inline_table_comment())
  }

  /// Whether the table should be printed over multiple lines, which only a TOML 1.1 parser
  /// accepts. A table the author wrote that way is kept that way unless it is asked to collapse,
  /// but one holding a comment anywhere within it has no choice.
  pub fn force_use_new_lines(&self, config: &Configuration) -> bool {
    self.contains_own_comment() || (!config.inline_table_prefer_single_line && self.multi_line_in_source)
  }
}
