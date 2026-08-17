// Reordering the tree, both for the `sort*` options and for the Cargo.toml conventions.
//
// Everything here treats a blank line as a divider the author put there deliberately: each run
// between blank lines is sorted on its own and runs never move past each other. Comments written
// above something travel with it, except for the comments heading a run, which stay at its top.

use std::cmp::Ordering;

use crate::ast::*;
use crate::configuration::Configuration;

/// Applies whichever of the sorting options are turned on.
pub fn apply_sorting(root: &mut Root, config: &Configuration) {
  if config.sort_keys {
    sort_root_keys(root);
  }
  if config.sort_arrays || config.sort_inline_tables {
    for item in &mut root.items {
      if let RootItem::Entry(entry) = item {
        sort_within_value(&mut entry.value, config);
      }
    }
  }
}

/// Sorts the entries of every table in the file, including the one above the first table header.
///
/// Table headers themselves are left where they are: moving one would change which entries belong
/// to it.
fn sort_root_keys(root: &mut Root) {
  let mut start = 0;
  while start <= root.items.len() {
    let end = section_end(&root.items, start);
    sort_root_entries(&mut root.items, start, end, &|left, right| compare_keys(&left.key, &right.key));
    // the header that ended the section isn't part of the next one
    start = end + 1;
  }
}

fn sort_within_value(value: &mut Value, config: &Configuration) {
  match &mut value.kind {
    ValueKind::Array(array) => {
      for item in &mut array.values {
        sort_within_value(&mut item.value, config);
      }
      // Only the text of a value decides where it sorts, so an array holding one that has no text
      // of its own — another array, or an inline table — is left alone rather than being
      // shuffled around an ordering that says nothing.
      if config.sort_arrays && array.values.iter().all(|item| value_sort_key(&item.value).is_some()) {
        sort_with_comments(&mut array.values, |left, right| value_sort_key(&left.value).cmp(&value_sort_key(&right.value)));
      }
    }
    ValueKind::InlineTable(table) => {
      for entry in &mut table.entries {
        sort_within_value(&mut entry.value, config);
      }
      if config.sort_inline_tables {
        sort_with_comments(&mut table.entries, |left, right| compare_keys(&left.key, &right.key));
      }
    }
    ValueKind::Scalar(_) | ValueKind::MultiLineString(_) => {}
  }
}

/// Compares two keys segment by segment, ignoring quoting so that `"serde"` sorts beside `serde`.
fn compare_keys(left: &Key, right: &Key) -> Ordering {
  let mut left = left.parts();
  let mut right = right.parts();
  loop {
    match (left.next(), right.next()) {
      (Some(left), Some(right)) => match left.unquoted_text().cmp(right.unquoted_text()) {
        Ordering::Equal => continue,
        ordering => return ordering,
      },
      (Some(_), None) => return Ordering::Greater,
      (None, Some(_)) => return Ordering::Less,
      (None, None) => return Ordering::Equal,
    }
  }
}

/// The text a value sorts under, or `None` for a value that is a collection rather than text.
///
/// A string sorts by its contents, so that the quote it happens to be written with -- which the
/// `quoteStyle` option may go on to change anyway — doesn't decide where it lands.
fn value_sort_key<'a>(value: &'a Value<'_>) -> Option<&'a str> {
  match &value.kind {
    ValueKind::Scalar(text) => Some(unquoted(text, &["\"", "'"])),
    ValueKind::MultiLineString(text) => Some(unquoted(text, &["\"\"\"", "'''"])),
    ValueKind::Array(_) | ValueKind::InlineTable(_) => None,
  }
}

fn unquoted<'a>(text: &'a str, quotes: &[&str]) -> &'a str {
  for quote in quotes {
    if let Some(inner) = text.strip_prefix(quote).and_then(|text| text.strip_suffix(quote)) {
      return inner;
    }
  }
  text
}

/// The index just past the last item belonging to the section starting at `start`, which runs until
/// the next table header.
pub fn section_end(items: &[RootItem], start: usize) -> usize {
  match items.get(start..) {
    Some(items_from_start) => items_from_start
      .iter()
      .position(RootItem::is_table_header)
      .map(|i| start + i)
      .unwrap_or(items.len()),
    None => items.len(),
  }
}

/// Something that can be sorted among its siblings, carrying the comments written above it.
pub trait Sortable<'a> {
  fn leading_comments(&self) -> &[Comment<'a>];
  fn take_leading_comments(&mut self) -> Vec<Comment<'a>>;
  fn set_leading_comments(&mut self, comments: Vec<Comment<'a>>);
  fn blank_line_before(&self) -> bool;
  fn set_blank_line_before(&mut self, value: bool);
}

impl<'a> Sortable<'a> for ArrayValue<'a> {
  fn leading_comments(&self) -> &[Comment<'a>] {
    &self.leading_comments
  }
  fn take_leading_comments(&mut self) -> Vec<Comment<'a>> {
    std::mem::take(&mut self.leading_comments)
  }
  fn set_leading_comments(&mut self, comments: Vec<Comment<'a>>) {
    self.leading_comments = comments;
  }
  fn blank_line_before(&self) -> bool {
    self.blank_line_before
  }
  fn set_blank_line_before(&mut self, value: bool) {
    self.blank_line_before = value;
  }
}

impl<'a> Sortable<'a> for Entry<'a> {
  fn leading_comments(&self) -> &[Comment<'a>] {
    &self.leading_comments
  }
  fn take_leading_comments(&mut self) -> Vec<Comment<'a>> {
    std::mem::take(&mut self.leading_comments)
  }
  fn set_leading_comments(&mut self, comments: Vec<Comment<'a>>) {
    self.leading_comments = comments;
  }
  fn blank_line_before(&self) -> bool {
    self.blank_line_before
  }
  fn set_blank_line_before(&mut self, value: bool) {
    self.blank_line_before = value;
  }
}

/// Sorts values that hold their own leading comments, such as the values of an array or the
/// entries of an inline table.
pub fn sort_with_comments<'a, T: Sortable<'a>>(values: &mut Vec<T>, cmp: impl Fn(&T, &T) -> Ordering) {
  let mut units = values
    .drain(..)
    .map(|mut value| Unit {
      leading_blank: value
        .leading_comments()
        .first()
        .map(|c| c.blank_line_before)
        .unwrap_or(value.blank_line_before()),
      inner_blank: !value.leading_comments().is_empty() && value.blank_line_before(),
      starts_group: value.blank_line_before() || value.leading_comments().iter().any(|c| c.blank_line_before),
      leading_comments: value.take_leading_comments(),
      value,
    })
    .collect::<Vec<_>>();
  sort_units(&mut units, cmp);
  values.extend(units.into_iter().map(|mut unit| {
    match unit.leading_comments.first_mut() {
      Some(first) => {
        first.blank_line_before = unit.leading_blank;
        unit.value.set_blank_line_before(unit.inner_blank);
      }
      None => unit.value.set_blank_line_before(unit.leading_blank),
    }
    unit.value.set_leading_comments(unit.leading_comments);
    unit.value
  }));
}

/// One sortable thing along with the comments written above it.
struct Unit<'a, T> {
  leading_comments: Vec<Comment<'a>>,
  /// Whether a blank line is rendered above this unit's first element.
  leading_blank: bool,
  /// Whether a blank line is rendered between this unit's comments and the item itself. Only
  /// meaningful when the unit has comments.
  inner_blank: bool,
  /// Whether a blank line appears anywhere between the previous item and this one, which the
  /// author uses to divide a section into groups. It may sit either above this unit's comments or
  /// between them and the item itself.
  starts_group: bool,
  value: T,
}

/// Sorts the entries of `items[start..end]`, keeping each entry's own comments with it.
pub fn sort_root_entries(items: &mut Vec<RootItem>, start: usize, end: usize, cmp: &impl Fn(&Entry, &Entry) -> Ordering) {
  // Split the section into one unit per entry, each carrying the comments written above it. Any
  // comments after the final entry belong to no entry and stay where they are.
  let mut units: Vec<Unit<Entry>> = Vec::new();
  let mut pending: Vec<Comment> = Vec::new();
  let mut trailing: Vec<Comment> = Vec::new();
  for item in items.drain(start..end) {
    match item {
      RootItem::Comment(comment) => pending.push(comment),
      RootItem::Entry(entry) => units.push(Unit {
        leading_blank: pending.first().map(|c| c.blank_line_before).unwrap_or(entry.blank_line_before),
        inner_blank: !pending.is_empty() && entry.blank_line_before,
        starts_group: entry.blank_line_before || pending.iter().any(|c| c.blank_line_before),
        leading_comments: std::mem::take(&mut pending),
        value: entry,
      }),
      // a section runs up to the next header, so nothing else can appear in it
      RootItem::TableHeader(_) => unreachable!(),
    }
  }
  trailing.append(&mut pending);

  sort_units(&mut units, |left, right| cmp(left, right));

  let sorted = units
    .into_iter()
    .flat_map(|unit| {
      let leading_blank = unit.leading_blank;
      let mut entry = unit.value;
      let mut items = Vec::with_capacity(unit.leading_comments.len() + 1);
      let mut comments = unit.leading_comments.into_iter();
      match comments.next() {
        Some(mut first) => {
          first.blank_line_before = leading_blank;
          items.push(RootItem::Comment(first));
          items.extend(comments.map(RootItem::Comment));
          entry.blank_line_before = unit.inner_blank;
        }
        None => entry.blank_line_before = leading_blank,
      }
      items.push(RootItem::Entry(entry));
      items
    })
    .chain(trailing.into_iter().map(RootItem::Comment))
    .collect::<Vec<_>>();
  items.splice(start..start, sorted);
}

/// Sorts `units` in place, treating a blank line as a divider the author put there deliberately:
/// each run between blank lines is sorted on its own and runs never move past each other.
///
/// The comments above a run's first entry are treated as belonging to the run rather than to that
/// entry, so a heading like `# exts` stays at the top of its run instead of being carried off to
/// wherever its entry happens to sort.
fn sort_units<T>(units: &mut [Unit<T>], cmp: impl Fn(&T, &T) -> Ordering) {
  let mut group_start = 0;
  while group_start < units.len() {
    let mut group_end = group_start + 1;
    while group_end < units.len() && !units[group_end].starts_group {
      group_end += 1;
    }

    let group = &mut units[group_start..group_end];
    // The group's leading comments belong to the group rather than to the entry they sat above, so
    // they stay at its top. Any blank line beneath them is part of that heading and travels with it.
    let group_comments = std::mem::take(&mut group[0].leading_comments);
    let leading_blank = group[0].leading_blank;
    let inner_blank = std::mem::replace(&mut group[0].inner_blank, false);

    group.sort_by(|left, right| cmp(&left.value, &right.value));

    for unit in group.iter_mut() {
      unit.leading_blank = false;
    }
    let head = &mut group[0];
    match head.leading_comments.first_mut() {
      // the blank now separates the group's heading from the comments of whichever entry sorted first
      Some(first) => first.blank_line_before = inner_blank,
      None => head.inner_blank = inner_blank,
    }
    head.leading_comments.splice(0..0, group_comments);
    head.leading_blank = leading_blank;

    group_start = group_end;
  }
}
