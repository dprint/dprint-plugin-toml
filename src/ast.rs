// AST types for the TOML parser.
//
// The tree is lossless for everything the formatter cares about: values keep their source text
// verbatim, and comments and blank lines are attached to the construct they belong to rather than
// being recovered by walking siblings. Anything not represented here is insignificant whitespace.

/// A byte-index range into the original text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
  pub start: usize,
  pub end: usize,
}

impl Span {
  pub fn new(start: usize, end: usize) -> Self {
    Span { start, end }
  }
}

/// A comment, from the `#` up to (but not including) the end of the line.
#[derive(Debug, Clone)]
pub struct Comment {
  /// The comment's source text, including the leading `#`.
  pub text: String,
  /// Whether a blank line separates this comment from whatever precedes it.
  pub blank_line_before: bool,
}

/// A parsed TOML document.
#[derive(Debug, Clone)]
pub struct Root {
  pub items: Vec<RootItem>,
}

/// A top level item. Comments on their own line are items in their own right rather than trivia
/// attached to the following item, which is what lets blank lines around them be preserved.
#[derive(Debug, Clone)]
pub enum RootItem {
  Comment(Comment),
  Entry(Entry),
  TableHeader(TableHeader),
}

impl RootItem {
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
pub struct TableHeader {
  pub key: Key,
  /// Whether this is an array of tables header (`[[key]]`).
  pub is_array_of_tables: bool,
  pub blank_line_before: bool,
  pub trailing_comment: Option<Comment>,
}

/// A key/value pair.
#[derive(Debug, Clone)]
pub struct Entry {
  pub key: Key,
  pub value: Value,
  pub blank_line_before: bool,
  pub trailing_comment: Option<Comment>,
  /// Comments on the lines immediately above this entry. Only populated for entries inside an
  /// inline table; at the top level such comments are separate [`RootItem::Comment`]s.
  pub leading_comments: Vec<Comment>,
}

/// A key, which may be dotted (`a.b.c`).
#[derive(Debug, Clone)]
pub struct Key {
  pub parts: Vec<KeyPart>,
}

impl Key {
  /// The key's source text with the insignificant whitespace around the dots removed.
  pub fn text(&self) -> String {
    self.parts.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join(".")
  }
}

/// One dot separated segment of a key. Bare, quoted and literal keys are all kept verbatim.
#[derive(Debug, Clone)]
pub struct KeyPart {
  pub text: String,
}

/// A value.
#[derive(Debug, Clone)]
pub struct Value {
  pub kind: ValueKind,
}

impl Value {
  /// Whether this value's own text spans more than one line, which happens only for a multi-line
  /// string. Those newlines are part of the value and can never be removed.
  pub fn contains_multi_line_string(&self) -> bool {
    match &self.kind {
      ValueKind::MultiLineString(_) => true,
      ValueKind::Scalar(_) => false,
      ValueKind::Array(array) => array.values.iter().any(|v| v.value.contains_multi_line_string()),
      ValueKind::InlineTable(table) => table.entries.iter().any(|e| e.value.contains_multi_line_string()),
    }
  }
}

#[derive(Debug, Clone)]
pub enum ValueKind {
  /// A single-line value kept verbatim: a string, number, boolean or date-time.
  Scalar(String),
  /// A multi-line basic or literal string, kept verbatim including its newlines.
  MultiLineString(String),
  Array(Array),
  InlineTable(InlineTable),
}

/// An array (`[1, 2, 3]`).
#[derive(Debug, Clone)]
pub struct Array {
  pub values: Vec<ArrayValue>,
  /// A comment on the same line as the opening bracket (`[ # here`).
  pub comment_after_open: Option<Comment>,
  /// Comments on their own lines between the last value and the closing bracket.
  pub comments_before_close: Vec<Comment>,
  /// Whether the opening bracket is followed by a newline or a comment, meaning the author wrote
  /// the array over multiple lines.
  pub multi_line_in_source: bool,
}

impl Array {
  /// Whether the array should be printed over multiple lines. An array the author broke up is kept
  /// broken up, but one that holds nothing at all collapses.
  pub fn force_use_new_lines(&self) -> bool {
    self.multi_line_in_source && !(self.values.is_empty() && self.comment_after_open.is_none() && self.comments_before_close.is_empty())
  }
}

/// A value within an array, along with the comments attached to it.
#[derive(Debug, Clone)]
pub struct ArrayValue {
  pub value: Value,
  /// Comments on the lines immediately above the value.
  pub leading_comments: Vec<Comment>,
  /// A comment on the same line as the value, after its comma if it has one.
  pub trailing_comment: Option<Comment>,
  pub blank_line_before: bool,
}

/// An inline table (`{ a = 1, b = 2 }`).
#[derive(Debug, Clone)]
pub struct InlineTable {
  pub entries: Vec<Entry>,
  /// A comment on the same line as the opening brace.
  pub comment_after_open: Option<Comment>,
  /// Comments on their own lines between the last entry and the closing brace.
  pub comments_before_close: Vec<Comment>,
  /// Whether the author wrote the table over multiple lines, which TOML 1.1 permits.
  pub multi_line_in_source: bool,
}
