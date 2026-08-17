// A hand written TOML parser producing the lossless tree in [`crate::ast`].
//
// It accepts TOML 1.1, which over 1.0 adds newlines and trailing commas in inline tables, the
// `\xHH` and `\e` escapes, and optional seconds in times. Whether any of that is *emitted* is the
// formatter's decision, not the parser's.
//
// Values are never interpreted, only delimited: the formatter reproduces them verbatim, so the
// parser's job for a value is to find where it ends. That keeps number, date-time and string
// handling to a scan rather than a full decode.

use crate::ast::*;

/// A byte-index range into the text being parsed.
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

/// A syntax error, with the span of the offending text.
#[derive(Debug, Clone)]
pub struct SyntaxError {
  pub message: String,
  pub span: Span,
}

/// Parses TOML text into a [`Root`].
pub fn parse(text: &str) -> Result<Root<'_>, SyntaxError> {
  let mut parser = Parser { text, pos: 0, depth: 0 };
  parser.parse_root()
}

/// Characters that end a bare value or key token.
fn is_value_terminator(c: char) -> bool {
  matches!(c, ' ' | '\t' | '\r' | '\n' | ',' | ']' | '}' | '#' | '=')
}

fn is_bare_key_char(c: char) -> bool {
  c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// How deeply arrays and inline tables may be nested within one another.
///
/// Each level costs a stack frame, and running out of stack aborts the process rather than raising
/// an error a host could catch — on wasm, where the plugin runs with a small stack and no guard
/// page, the overflow isn't even detected. Real TOML nests a handful of levels deep, so a limit
/// well below where the stack runs out turns the crash into a syntax error and costs nothing.
const MAX_NESTING_DEPTH: usize = 128;

struct Parser<'a> {
  text: &'a str,
  pos: usize,
  /// How many arrays and inline tables enclose the value being parsed.
  depth: usize,
}

impl<'a> Parser<'a> {
  // ---- scanning primitives ----

  fn rest(&self) -> &'a str {
    &self.text[self.pos..]
  }

  fn is_eof(&self) -> bool {
    self.pos >= self.text.len()
  }

  fn peek(&self) -> Option<char> {
    self.rest().chars().next()
  }

  fn starts_with(&self, s: &str) -> bool {
    self.rest().starts_with(s)
  }

  fn bump(&mut self) -> Option<char> {
    let c = self.peek()?;
    self.pos += c.len_utf8();
    Some(c)
  }

  fn skip_spaces(&mut self) {
    while matches!(self.peek(), Some(' ') | Some('\t')) {
      self.pos += 1;
    }
  }

  /// Consumes a `\n` or `\r\n`, reporting whether one was there.
  fn try_skip_newline(&mut self) -> bool {
    if self.starts_with("\r\n") {
      self.pos += 2;
      true
    } else if self.starts_with("\n") {
      self.pos += 1;
      true
    } else {
      false
    }
  }

  fn error<T>(&self, message: impl Into<String>, span: Span) -> Result<T, SyntaxError> {
    Err(SyntaxError { message: message.into(), span })
  }

  fn error_here<T>(&self, message: impl Into<String>) -> Result<T, SyntaxError> {
    let end = match self.peek() {
      Some(c) => self.pos + c.len_utf8(),
      None => self.pos,
    };
    self.error(message, Span::new(self.pos, end))
  }

  // ---- comments ----

  /// Parses a comment, which runs to the end of the line. The terminating newline is left alone.
  fn parse_comment(&mut self, blank_line_before: bool) -> Comment<'a> {
    let start = self.pos;
    while let Some(c) = self.peek() {
      if c == '\n' || c == '\r' {
        break;
      }
      self.bump();
    }
    Comment {
      // trailing spaces and tabs on a comment line are insignificant, but `trim_end` would also
      // take Unicode whitespace such as a no-break space, which is legitimate comment content
      text: self.text[start..self.pos].trim_end_matches([' ', '\t']),
      blank_line_before,
    }
  }

  /// Consumes spaces and, if a comment follows on the same line, parses it.
  fn parse_trailing_comment(&mut self) -> Option<Comment<'a>> {
    self.skip_spaces();
    if self.peek() == Some('#') {
      Some(self.parse_comment(false))
    } else {
      None
    }
  }

  // ---- root ----

  fn parse_root(&mut self) -> Result<Root<'a>, SyntaxError> {
    let mut items = Vec::new();
    let mut newlines = 0usize;
    loop {
      self.skip_spaces();
      if self.is_eof() {
        break;
      }
      if self.try_skip_newline() {
        newlines += 1;
        continue;
      }
      if self.peek() == Some('\r') {
        return self.error_here("expected a line feed after the carriage return");
      }

      // the newline that ends the previous item's line counts as one, so a blank line is two
      let blank_line_before = !items.is_empty() && newlines >= 2;
      newlines = 0;

      match self.peek() {
        Some('#') => {
          items.push(RootItem::Comment(self.parse_comment(blank_line_before)));
          continue;
        }
        Some('[') => items.push(RootItem::TableHeader(self.parse_table_header(blank_line_before)?)),
        _ => {
          let mut entry = self.parse_entry(blank_line_before, Vec::new())?;
          entry.trailing_comment = self.parse_trailing_comment();
          items.push(RootItem::Entry(entry));
        }
      }

      // an item and its trailing comment have to be the last thing on their line
      if !self.is_eof() && !matches!(self.peek(), Some('\n') | Some('\r')) {
        return self.error_here("expected a newline");
      }
    }
    Ok(Root { items })
  }

  fn parse_table_header(&mut self, blank_line_before: bool) -> Result<TableHeader<'a>, SyntaxError> {
    self.bump(); // '['
    let is_array_of_tables = self.peek() == Some('[');
    if is_array_of_tables {
      self.bump();
    }

    self.skip_spaces();
    let key = self.parse_key()?;
    self.skip_spaces();

    let closing = if is_array_of_tables { "]]" } else { "]" };
    if !self.starts_with(closing) {
      return self.error_here(format!("expected '{closing}' to close the table header"));
    }
    self.pos += closing.len();

    Ok(TableHeader {
      key,
      is_array_of_tables,
      blank_line_before,
      trailing_comment: self.parse_trailing_comment(),
    })
  }

  fn parse_entry(&mut self, blank_line_before: bool, leading_comments: Vec<Comment<'a>>) -> Result<Entry<'a>, SyntaxError> {
    let key = self.parse_key()?;
    self.skip_spaces();
    if self.peek() != Some('=') {
      return self.error_here("expected '=' after the key");
    }
    self.bump();
    self.skip_spaces();
    let value = self.parse_value()?;

    Ok(Entry {
      key,
      value,
      blank_line_before,
      trailing_comment: None, // filled in by the caller, which knows where the line ends
      leading_comments,
    })
  }

  // ---- keys ----

  fn parse_key(&mut self) -> Result<Key<'a>, SyntaxError> {
    self.skip_spaces();
    let first = self.parse_key_part()?;
    // a dotted key is the exception, so `rest` stays empty and unallocated for most keys
    let mut rest = Vec::new();
    loop {
      self.skip_spaces();
      if self.peek() != Some('.') {
        break;
      }
      self.bump();
      self.skip_spaces();
      rest.push(self.parse_key_part()?);
    }
    Ok(Key { first, rest })
  }

  fn parse_key_part(&mut self) -> Result<KeyPart<'a>, SyntaxError> {
    let start = self.pos;
    if self.starts_with("\"\"\"") || self.starts_with("'''") {
      return self.error_here("a key cannot be a multi-line string");
    }
    match self.peek() {
      Some('"') => self.scan_basic_string()?,
      Some('\'') => self.scan_literal_string()?,
      Some(c) if is_bare_key_char(c) => {
        while matches!(self.peek(), Some(c) if is_bare_key_char(c)) {
          self.bump();
        }
      }
      _ => return self.error_here("expected a key"),
    }
    Ok(KeyPart {
      text: &self.text[start..self.pos],
    })
  }

  /// Parses a collection one level further in, refusing to recurse past [`MAX_NESTING_DEPTH`].
  fn parse_nested<T>(&mut self, parse: impl FnOnce(&mut Self) -> Result<T, SyntaxError>) -> Result<T, SyntaxError> {
    if self.depth >= MAX_NESTING_DEPTH {
      return self.error_here("arrays and inline tables are nested too deeply");
    }
    self.depth += 1;
    let result = parse(self);
    self.depth -= 1;
    result
  }

  // ---- values ----

  fn parse_value(&mut self) -> Result<Value<'a>, SyntaxError> {
    let start = self.pos;
    match self.peek() {
      Some('[') => {
        let array = self.parse_nested(Self::parse_array)?;
        Ok(Value { kind: ValueKind::Array(array) })
      }
      Some('{') => {
        let table = self.parse_nested(Self::parse_inline_table)?;
        Ok(Value {
          kind: ValueKind::InlineTable(table),
        })
      }
      Some('"') | Some('\'') => {
        let is_multi_line = self.starts_with("\"\"\"") || self.starts_with("'''");
        if self.peek() == Some('"') {
          self.scan_basic_string()?;
        } else {
          self.scan_literal_string()?;
        }
        let text = &self.text[start..self.pos];
        Ok(Value {
          kind: if is_multi_line {
            ValueKind::MultiLineString(text)
          } else {
            ValueKind::Scalar(text)
          },
        })
      }
      Some(_) => {
        self.scan_bare_value()?;
        Ok(Value {
          kind: ValueKind::Scalar(&self.text[start..self.pos]),
        })
      }
      None => self.error_here("expected a value"),
    }
  }

  /// Scans an unquoted value: a number, boolean or date-time.
  fn scan_bare_value(&mut self) -> Result<(), SyntaxError> {
    let start = self.pos;
    while let Some(c) = self.peek() {
      if is_value_terminator(c) {
        break;
      }
      self.bump();
    }
    if self.pos == start {
      return self.error_here("expected a value");
    }

    // A date-time may be written with a space between its date and time parts, which would
    // otherwise have ended the token: `1979-05-27 07:32:00Z`.
    if is_date(&self.text[start..self.pos]) && self.peek() == Some(' ') {
      let after_space = &self.rest()[1..];
      if starts_with_time(after_space) {
        self.bump(); // the space
        while let Some(c) = self.peek() {
          if is_value_terminator(c) {
            break;
          }
          self.bump();
        }
      }
    }

    if !is_bare_value_shaped(&self.text[start..self.pos]) {
      return self.error("expected a value", Span::new(start, self.pos));
    }
    Ok(())
  }

  // ---- strings ----

  /// Scans a `"` delimited string, single or multi-line, leaving the position after it.
  fn scan_basic_string(&mut self) -> Result<(), SyntaxError> {
    let start = self.pos;
    if self.starts_with("\"\"\"") {
      self.pos += 3;
      return self.scan_multi_line_string(start, '"', true);
    }
    self.bump(); // opening quote
    loop {
      match self.peek() {
        None | Some('\n') | Some('\r') => return self.error("unterminated string", Span::new(start, self.pos)),
        Some('\\') => {
          self.bump();
          // Whatever follows an escape is part of the string, including a quote. A newline is not:
          // the line-ending backslash belongs to multi-line strings only, and letting one through
          // would put a newline inside a value the formatter prints on a single line.
          match self.bump() {
            Some('\n') | Some('\r') | None => return self.error("unterminated string", Span::new(start, self.pos)),
            Some(_) => {}
          }
        }
        Some('"') => {
          self.bump();
          return Ok(());
        }
        _ => {
          self.bump();
        }
      }
    }
  }

  /// Scans a `'` delimited string, single or multi-line, leaving the position after it.
  fn scan_literal_string(&mut self) -> Result<(), SyntaxError> {
    let start = self.pos;
    if self.starts_with("'''") {
      self.pos += 3;
      return self.scan_multi_line_string(start, '\'', false);
    }
    self.bump(); // opening quote
    loop {
      match self.peek() {
        // a literal string has no escapes, so the first quote ends it
        None | Some('\n') | Some('\r') => return self.error("unterminated string", Span::new(start, self.pos)),
        Some('\'') => {
          self.bump();
          return Ok(());
        }
        _ => {
          self.bump();
        }
      }
    }
  }

  /// Scans the body of a multi-line string, having already consumed its opening delimiter.
  ///
  /// The delimiter is three quotes, but one or two more may sit against it and belong to the
  /// string's contents, as in `""""quoted""""`. Since at most two quotes can appear in a row
  /// inside the string, any run of three or more both ends it and takes up to two of those quotes
  /// as content.
  fn scan_multi_line_string(&mut self, start: usize, quote: char, has_escapes: bool) -> Result<(), SyntaxError> {
    loop {
      match self.peek() {
        None => return self.error("unterminated multi-line string", Span::new(start, self.pos)),
        Some('\\') if has_escapes => {
          self.bump();
          if self.bump().is_none() {
            return self.error("unterminated multi-line string", Span::new(start, self.pos));
          }
        }
        Some(c) if c == quote => {
          let run_start = self.pos;
          while self.peek() == Some(quote) {
            self.bump();
          }
          let run_len = self.pos - run_start;
          if run_len > 5 {
            // three close the string and at most two more can be content, so this can't terminate
            return self.error(
              format!("too many {quote:?} in a row to end a multi-line string"),
              Span::new(run_start, self.pos),
            );
          }
          if run_len >= 3 {
            // at most two of the run's quotes are content; the rest closes the string
            self.pos = run_start + run_len;
            return Ok(());
          }
        }
        _ => {
          self.bump();
        }
      }
    }
  }

  // ---- arrays ----

  fn parse_array(&mut self) -> Result<Array<'a>, SyntaxError> {
    let start = self.pos;
    self.bump(); // '['

    // Only what directly follows the bracket decides this, so a collection whose first item sits
    // on the opening line is not considered multi-line however its later items were laid out.
    let comment_after_open = self.parse_trailing_comment();
    let multi_line_in_source = comment_after_open.is_some() || matches!(self.peek(), Some('\n') | Some('\r'));

    let mut values: Vec<ArrayValue<'a>> = Vec::new();
    let mut pending_comments: Vec<Comment<'a>> = Vec::new();
    let mut newlines = 0usize;
    let mut separated = true;

    loop {
      self.skip_spaces();
      match self.peek() {
        None => return self.error("unterminated array", Span::new(start, self.pos)),
        Some('\n') | Some('\r') => {
          // a lone carriage return isn't a newline, and leaving it would spin here forever
          if !self.try_skip_newline() {
            return self.error_here("expected a line feed after the carriage return");
          }
          newlines += 1;
          continue;
        }
        Some('#') => {
          let has_preceding = !values.is_empty() || !pending_comments.is_empty() || comment_after_open.is_some();
          let comment = self.parse_comment(has_preceding && newlines >= 2);
          pending_comments.push(comment);
          newlines = 0;
          continue;
        }
        Some(',') if !separated => {
          // A comma is allowed to trail onto a later line than the value it follows. The newlines
          // it sat behind belong to that value's line rather than to whatever comes next, so any
          // blank among them is deliberately dropped rather than moved onto the next value.
          self.bump();
          separated = true;
          newlines = 0;
          if let Some(comment) = self.parse_trailing_comment() {
            match values.last_mut() {
              Some(value) if value.trailing_comment.is_none() => value.trailing_comment = Some(comment),
              _ => pending_comments.push(comment),
            }
          }
          continue;
        }
        Some(']') => {
          self.bump();
          break;
        }
        _ => {}
      }

      if !separated {
        return self.error_here("expected ',' between array values");
      }
      // measured after any leading comments, so it means a blank between them and the value
      let blank_line_before = newlines >= 2;
      newlines = 0;
      let value = self.parse_value()?;

      // a comma may follow, and a comment may follow either the value or the comma
      let mut trailing_comment = self.parse_trailing_comment();
      self.skip_spaces();
      separated = self.peek() == Some(',');
      if separated {
        self.bump();
        if trailing_comment.is_none() {
          trailing_comment = self.parse_trailing_comment();
        }
      }

      values.push(ArrayValue {
        value,
        leading_comments: std::mem::take(&mut pending_comments),
        trailing_comment,
        blank_line_before,
      });
    }

    Ok(Array {
      values,
      comment_after_open,
      comments_before_close: pending_comments,
      multi_line_in_source,
    })
  }

  // ---- inline tables ----

  fn parse_inline_table(&mut self) -> Result<InlineTable<'a>, SyntaxError> {
    let start = self.pos;
    self.bump(); // '{'

    // Only what directly follows the bracket decides this, so a collection whose first item sits
    // on the opening line is not considered multi-line however its later items were laid out.
    let comment_after_open = self.parse_trailing_comment();
    let multi_line_in_source = comment_after_open.is_some() || matches!(self.peek(), Some('\n') | Some('\r'));

    let mut entries: Vec<Entry<'a>> = Vec::new();
    let mut pending_comments: Vec<Comment<'a>> = Vec::new();
    let mut newlines = 0usize;
    let mut separated = true;

    loop {
      self.skip_spaces();
      match self.peek() {
        None => return self.error("unterminated inline table", Span::new(start, self.pos)),
        Some('\n') | Some('\r') => {
          // TOML 1.1 permits newlines within an inline table. A lone carriage return isn't one,
          // and leaving it would spin here forever.
          if !self.try_skip_newline() {
            return self.error_here("expected a line feed after the carriage return");
          }
          newlines += 1;
          continue;
        }
        Some('#') => {
          let has_preceding = !entries.is_empty() || !pending_comments.is_empty() || comment_after_open.is_some();
          let comment = self.parse_comment(has_preceding && newlines >= 2);
          pending_comments.push(comment);
          newlines = 0;
          continue;
        }
        Some(',') if !separated => {
          self.bump();
          separated = true;
          newlines = 0;
          if let Some(comment) = self.parse_trailing_comment() {
            match entries.last_mut() {
              Some(entry) if entry.trailing_comment.is_none() => entry.trailing_comment = Some(comment),
              _ => pending_comments.push(comment),
            }
          }
          continue;
        }
        Some('}') => {
          self.bump();
          break;
        }
        _ => {}
      }

      if !separated {
        return self.error_here("expected ',' between inline table entries");
      }
      // measured after any leading comments, so it means a blank between them and the entry
      let blank_line_before = newlines >= 2;
      newlines = 0;
      let mut entry = self.parse_entry(blank_line_before, std::mem::take(&mut pending_comments))?;

      let mut trailing_comment = self.parse_trailing_comment();
      self.skip_spaces();
      separated = self.peek() == Some(',');
      if separated {
        self.bump();
        if trailing_comment.is_none() {
          trailing_comment = self.parse_trailing_comment();
        }
      }
      entry.trailing_comment = trailing_comment;
      entries.push(entry);
    }

    Ok(InlineTable {
      entries,
      comment_after_open,
      comments_before_close: pending_comments,
      multi_line_in_source,
    })
  }
}

/// Whether the text could be an unquoted TOML value.
///
/// This is a shape check rather than a full parse: the formatter reproduces values verbatim, so it
/// only needs to know that a token isn't something like `foo = BAR`. Every valid unquoted value is
/// a keyword or begins with a digit or a sign, so nothing valid is turned away here. Whether the
/// digits themselves form a real number or date-time is left to whatever reads the TOML.
fn is_bare_value_shaped(text: &str) -> bool {
  if matches!(text, "true" | "false" | "inf" | "+inf" | "-inf" | "nan" | "+nan" | "-nan") {
    return true;
  }
  let starts_ok = matches!(text.chars().next(), Some(c) if c.is_ascii_digit() || c == '+' || c == '-');
  // the space is only ever reached through a date-time's date/time separator
  starts_ok
    && text
      .chars()
      .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '.' | '-' | '+' | ' '))
}

/// Whether the text is exactly a `YYYY-MM-DD` date.
fn is_date(text: &str) -> bool {
  let bytes = text.as_bytes();
  bytes.len() == 10
    && bytes[0..4].iter().all(u8::is_ascii_digit)
    && bytes[4] == b'-'
    && bytes[5..7].iter().all(u8::is_ascii_digit)
    && bytes[7] == b'-'
    && bytes[8..10].iter().all(u8::is_ascii_digit)
}

/// Whether the text begins with an `HH:MM` time.
fn starts_with_time(text: &str) -> bool {
  let bytes = text.as_bytes();
  bytes.len() >= 5 && bytes[0..2].iter().all(u8::is_ascii_digit) && bytes[2] == b':' && bytes[3..5].iter().all(u8::is_ascii_digit)
}
