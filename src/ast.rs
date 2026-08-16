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

/// A comment, from the `#` up to (but not including) the end of the line.
#[derive(Debug, Clone)]
pub struct Comment<'a> {
  /// The comment's source text, including the leading `#`.
  pub text: &'a str,
  /// Whether a blank line separates this comment from whatever precedes it.
  pub blank_line_before: bool,
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

  /// Whether a comment appears anywhere within this value.
  pub fn contains_comment(&self) -> bool {
    match &self.kind {
      ValueKind::Scalar(_) | ValueKind::MultiLineString(_) => false,
      ValueKind::Array(array) => {
        array.comment_after_open.is_some()
          || !array.comments_before_close.is_empty()
          || array
            .values
            .iter()
            .any(|value| value.trailing_comment.is_some() || !value.leading_comments.is_empty() || value.value.contains_comment())
      }
      ValueKind::InlineTable(table) => table.contains_comment(),
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
  pub fn force_use_new_lines(&self) -> bool {
    // A comment written on its own line before the closing bracket is only given a line of its own
    // when the array is broken up. Left on one line it is printed against the last value, turning
    // it into that value's trailing comment and formatting differently the second time around.
    if !self.comments_before_close.is_empty() {
      return true;
    }
    self.multi_line_in_source && !(self.values.is_empty() && self.comment_after_open.is_none())
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
  /// Whether a comment appears anywhere within this table, including inside its values.
  ///
  /// A comment runs to the end of its line, so one can only sit inside braces that already hold a
  /// newline. That makes such a table multi-line in the source even when `multi_line_in_source` is
  /// false, since that field records only what directly follows the opening brace.
  pub fn contains_comment(&self) -> bool {
    self.comment_after_open.is_some()
      || !self.comments_before_close.is_empty()
      || self
        .entries
        .iter()
        .any(|entry| entry.trailing_comment.is_some() || !entry.leading_comments.is_empty() || entry.value.contains_comment())
  }
}
