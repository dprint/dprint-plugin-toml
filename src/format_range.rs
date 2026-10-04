use std::borrow::Cow;
use std::ops::Range;
use std::ops::RangeInclusive;
use std::path::Path;

use dprint_core::configuration::resolve_new_line_kind;
use dprint_core::configuration::NewLineKind;

use super::configuration::Configuration;
use crate::ast::Root;
use crate::ast::RootItem;
use crate::error::FormatError;
use crate::format_text::format_text;
use crate::format_text::format_text_inner;
use crate::format_text::parse;
use crate::format_text::print;
use crate::format_text::process_node;
use crate::format_text::strip_bom;
use crate::sorting::section_end;

/// Formats only the part of the text within the provided byte range.
///
/// The range is widened to the lines of the entries, table headers and comments it touches at the
/// top level of the file, and the text outside of those is left as it was, including the blank
/// lines above and below them, the line endings and any byte order mark. An empty range is a
/// cursor, which touches whatever is on its line.
///
/// When sorting moves any of those (ex. the `Cargo.toml` conventions or the `sortKeys` option),
/// the lines they are moved within are formatted instead. When formatting the rest of the file
/// afterwards wouldn't then give what formatting all of it at once does (ex. a table header and
/// the first entry beneath it decide between them whether the table's entries are indented, so
/// changing the indentation of only one changes the answer), the tables they are in are formatted
/// instead, and failing that the whole file. A range that touches both the first and last of the
/// file's entries, table headers and comments formats the whole file and one that only touches
/// blank lines formats nothing.
pub fn format_text_range(file_path: &Path, text: &str, range: Range<usize>, config: &Configuration) -> Result<Option<String>, FormatError> {
  let body = strip_bom(text);
  let bom_len = text.len() - body.len();
  let end = range.end.saturating_sub(bom_len).min(body.len());
  let range = range.start.saturating_sub(bom_len).min(end)..end;
  // also what formats a file that only has blank lines, which has nothing to touch
  if range.start == 0 && range.end == body.len() {
    return format_text(file_path, text, config);
  }

  let mut root = parse(body)?;
  let starts = item_starts(&root);
  let Some(touched) = find_touched(body, &starts, &range) else {
    return Ok(None);
  };

  process_node(file_path, &mut root, config);
  let formatted = print(&root, body, config);
  let formatted_starts = item_starts(&parse(&formatted)?);
  debug_assert_eq!(formatted_starts.len(), starts.len());
  let moved = widen_to_moved(&root, &starts, touched);
  let tables = widen_to_tables(&root, moved.clone());
  let mut result = None;
  for items in [moved, tables] {
    // the blank lines at the start of the file and the newline at its end belong to no item, so
    // they are only formatted along with all of it
    if *items.start() == 0 && *items.end() == starts.len() - 1 {
      break;
    }
    let original = lines_range(body, item_range(body, &starts, items.clone()));
    let replacement = lines_range(&formatted, item_range(&formatted, &formatted_starts, items));
    let spliced = format!(
      "{}{}{}",
      &body[..original.start],
      with_file_new_lines(&formatted[replacement], body),
      &body[original.end..]
    );
    if format_text_inner(file_path, &spliced, config)? == formatted {
      result = Some(format!("{}{}", &text[..bom_len], spliced));
      break;
    }
  }
  let result = result.unwrap_or(formatted);
  if result == text {
    Ok(None)
  } else {
    Ok(Some(result))
  }
}

/// Where each of the root's items starts in the text it was parsed from.
fn item_starts(root: &Root) -> Vec<usize> {
  root.items.iter().map(RootItem::start_in_source).collect()
}

/// The indexes of the items the range touches, if any.
///
/// A cursor touches an item from anywhere on its lines, but a selection needs to hold some of the
/// item itself, so that one ending in the indentation of the next line doesn't format that line.
fn find_touched(text: &str, starts: &[usize], range: &Range<usize>) -> Option<RangeInclusive<usize>> {
  let mut touched = (0..starts.len()).filter(|index| {
    let item = item_range(text, starts, *index..=*index);
    if range.is_empty() {
      let lines = lines_range(text, item);
      lines.start <= range.start && range.start <= lines.end
    } else {
      range.start < item.end && range.end > item.start
    }
  });
  let first = touched.next()?;
  let last = touched.next_back().unwrap_or(first);
  Some(first..=last)
}

/// Widens the items until the same ones are within them once sorted, whatever order they end up
/// in, since the formatted items can only be put back in place of the original ones when they're
/// the same ones.
///
/// `root` is the file once sorted and `starts` is where each item started before it was.
fn widen_to_moved(root: &Root, starts: &[usize], items: RangeInclusive<usize>) -> RangeInclusive<usize> {
  let (mut first, mut last) = items.into_inner();
  loop {
    let original_indexes = (first..=last).map(|index| starts.binary_search(&root.items[index].start_in_source()).unwrap_or(index));
    let (new_first, new_last) = original_indexes.fold((first, last), |(first, last), index| (first.min(index), last.max(index)));
    if (new_first, new_last) == (first, last) {
      return first..=last;
    }
    (first, last) = (new_first, new_last);
  }
}

/// Widens the items to all of the tables they are in, from the header of the first to the last
/// entry of the last.
fn widen_to_tables(root: &Root, items: RangeInclusive<usize>) -> RangeInclusive<usize> {
  let (first, last) = items.into_inner();
  let first = root.items[..=first].iter().rposition(RootItem::is_table_header).unwrap_or(0);
  first..=section_end(&root.items, last + 1) - 1
}

/// The text from the start of the first item to the end of the last one.
///
/// Only whitespace separates one item from the next since a comment is an item too.
fn item_range(text: &str, starts: &[usize], items: RangeInclusive<usize>) -> Range<usize> {
  let start = starts[*items.start()];
  let next_start = starts.get(items.end() + 1).copied().unwrap_or(text.len());
  start..start + text[start..next_start].trim_end_matches([' ', '\t', '\r', '\n']).len()
}

/// Widens the range to the start of its first line, so that the indentation there is formatted,
/// and to the end of its last line without the line break.
fn lines_range(text: &str, range: Range<usize>) -> Range<usize> {
  let start = text[..range.start].rfind('\n').map(|index| index + 1).unwrap_or(0);
  let rest = &text[range.end..];
  let line = rest.find('\n').map(|index| &rest[..index]).unwrap_or(rest);
  start..range.end + line.trim_end_matches('\r').len()
}

/// Keeps the line endings of the rest of the file since changing those is up to formatting the
/// whole file.
fn with_file_new_lines<'a>(formatted: &'a str, file_text: &str) -> Cow<'a, str> {
  if !file_text.contains('\n') {
    return Cow::Borrowed(formatted);
  }
  match resolve_new_line_kind(file_text, NewLineKind::Auto) {
    "\r\n" if formatted.contains('\n') && !formatted.contains("\r\n") => Cow::Owned(formatted.replace('\n', "\r\n")),
    "\n" if formatted.contains("\r\n") => Cow::Owned(formatted.replace("\r\n", "\n")),
    _ => Cow::Borrowed(formatted),
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::configuration::ConfigurationBuilder;

  #[test]
  fn keeps_bom_outside_range() {
    // the spec files can't express this since editors strip the bom
    let config = ConfigurationBuilder::new().build();
    let output = format_b(&config, "\u{FEFF}a   =   1\nb   =   2\nc   =   3\n");
    assert_eq!(output, "\u{FEFF}a   =   1\nb = 2\nc   =   3\n");
  }

  #[test]
  fn keeps_file_line_endings() {
    // the spec files can't express this since they normalize line endings
    let config = ConfigurationBuilder::new().build();
    let output = format_b(&config, "a   =   1\r\nb   =   [\r\n1]  \r\nc   =   3\r\n");
    assert_eq!(output, "a   =   1\r\nb = [\r\n  1,\r\n]\r\nc   =   3\r\n");
  }

  #[test]
  fn keeps_file_line_endings_over_configured_ones() {
    let config = ConfigurationBuilder::new().new_line_kind(NewLineKind::CarriageReturnLineFeed).build();
    let output = format_b(&config, "a   =   1\nb   =   [\n1]\nc   =   3\n");
    assert_eq!(output, "a   =   1\nb = [\n  1,\n]\nc   =   3\n");
  }

  #[test]
  fn keeps_file_line_endings_in_multi_line_string() {
    let config = ConfigurationBuilder::new().build();
    let output = format_b(&config, "a   =   1\r\nb   =   \"\"\"\r\n  x\r\n\"\"\"\r\nc   =   3\r\n");
    assert_eq!(output, "a   =   1\r\nb = \"\"\"\r\n  x\r\n\"\"\"\r\nc   =   3\r\n");
  }

  #[test]
  fn removes_trailing_whitespace_of_touched_lines() {
    // the spec files can't express this since editors strip trailing whitespace
    let config = ConfigurationBuilder::new().build();
    let output = format_b(&config, "a = 1  \nb = 2 \t \nc = 3  \n");
    assert_eq!(output, "a = 1  \nb = 2\nc = 3  \n");
  }

  #[test]
  fn formats_item_cursor_is_in_trailing_whitespace_of() {
    let config = ConfigurationBuilder::new().build();
    let text = "a   =   1\nb   =   2   \nc   =   3\n";
    let cursor = text.find("   \n").unwrap() + 3;
    let output = format_text_range(Path::new("file.toml"), text, cursor..cursor, &config).unwrap();
    assert_eq!(output.as_deref(), Some("a   =   1\nb = 2\nc   =   3\n"));
  }

  #[test]
  fn formats_range_that_includes_bom() {
    let config = ConfigurationBuilder::new().build();
    let text = "\u{FEFF}a   =   1\nb   =   2\n";
    let output = format_text_range(Path::new("file.toml"), text, 0..4, &config).unwrap();
    assert_eq!(output.as_deref(), Some("\u{FEFF}a = 1\nb   =   2\n"));
  }

  #[test]
  fn formats_last_item_without_adding_final_newline() {
    let config = ConfigurationBuilder::new().build();
    let output = format_b(&config, "a   =   1\nb   =   2");
    assert_eq!(output, "a   =   1\nb = 2");
  }

  #[test]
  fn does_not_format_at_end_of_file() {
    let config = ConfigurationBuilder::new().build();
    let text = "a   =   1\nb   =   2\n";
    let output = format_text_range(Path::new("file.toml"), text, text.len()..text.len(), &config).unwrap();
    assert_eq!(output, None);
  }

  #[test]
  fn does_not_format_when_already_formatted() {
    let config = ConfigurationBuilder::new().build();
    let output = format_text_range(Path::new("file.toml"), "a   =   1\nb = 2\nc   =   3\n", 10..11, &config).unwrap();
    assert_eq!(output, None);
  }

  #[test]
  fn formats_file_with_only_blank_lines_when_range_is_all_of_it() {
    let config = ConfigurationBuilder::new().build();
    let output = format_text_range(Path::new("file.toml"), "\n\n\n", 0..3, &config).unwrap();
    assert_eq!(output, format_text(Path::new("file.toml"), "\n\n\n", &config).unwrap());
    let output = format_text_range(Path::new("file.toml"), "\n\n\n", 1..2, &config).unwrap();
    assert_eq!(output, None);
  }

  #[test]
  fn errors_for_syntax_error_outside_range() {
    let config = ConfigurationBuilder::new().build();
    let result = format_text_range(Path::new("file.toml"), "a   =   1\nb = \n", 0..1, &config);
    assert!(result.unwrap_err().to_string().contains("expected a value"));
  }

  #[test]
  fn formats_whole_file_when_touching_first_and_last_items() {
    let config = ConfigurationBuilder::new().build();
    let output = format_text_range(Path::new("file.toml"), "\n\na   =   1\nb   =   2", 3..14, &config).unwrap();
    assert_eq!(output.as_deref(), Some("a = 1\nb = 2\n"));
  }

  #[test]
  fn clamps_range_past_end_of_file() {
    let config = ConfigurationBuilder::new().build();
    let text = "a   =   1\nb   =   2\n\n";
    let output = format_text_range(Path::new("file.toml"), text, 12..500, &config).unwrap();
    assert_eq!(output.as_deref(), Some("a   =   1\nb = 2\n\n"));
    let output = format_text_range(Path::new("file.toml"), text, 400..500, &config).unwrap();
    assert_eq!(output, None);
  }

  fn format_b(config: &Configuration, text: &str) -> String {
    let start = text.find('b').unwrap();
    format_text_range(Path::new("file.toml"), text, start..start + 1, config).unwrap().unwrap()
  }
}
