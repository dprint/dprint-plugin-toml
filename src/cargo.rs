use std::cmp::Ordering;
use std::path::Path;

use crate::ast::*;

pub fn is_cargo_toml_file(file_path: &Path) -> bool {
  // don't need to worry about different casing because Cargo.toml will
  // always have this same casing https://github.com/rust-lang/cargo/issues/45
  file_path.file_name().map(|n| n == "Cargo.toml").unwrap_or(false)
}

/// A table whose contents the conventions rearrange, plus `[workspace]`, whose `members` entry
/// they rearrange.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
  Package,
  Dependencies,
  Workspace,
  Other,
}

fn section_of(header: &TableHeader) -> Section {
  // an array of tables holds repeated elements rather than one table's keys, so it is never a
  // section the conventions sort, whatever it is named
  if header.is_array_of_tables {
    return Section::Other;
  }
  let key = &header.key;
  if key.names("package") || key.names("workspace.package") {
    Section::Package
  } else if key.names("dependencies") || key.names("dev-dependencies") || key.names("workspace.dependencies") {
    Section::Dependencies
  } else if key.names("workspace") {
    Section::Workspace
  } else {
    Section::Other
  }
}

pub fn apply_cargo_toml_conventions(root: &mut Root) {
  let mut index = 0;
  let mut last_header = Section::Other;

  while index < root.items.len() {
    match &root.items[index] {
      RootItem::TableHeader(header) => {
        let section = section_of(header);
        let end = section_end(&root.items, index + 1);
        match section {
          Section::Package => sort_section(&mut root.items, index + 1, end, &sort_cargo_package_section),
          Section::Dependencies => sort_section(&mut root.items, index + 1, end, &|left, right| entry_sort_key(left).cmp(entry_sort_key(right))),
          Section::Workspace | Section::Other => {}
        }
        last_header = section;
        // sorting a section doesn't change how many items it has, so the walk just continues
        index += 1;
      }
      RootItem::Entry(entry) => {
        if last_header == Section::Workspace && entry_sort_key(entry) == "members" {
          if let RootItem::Entry(entry) = &mut root.items[index] {
            sort_workspace_members(entry);
          }
        }
        index += 1;
      }
      RootItem::Comment(_) => index += 1,
    }
  }
}

/// The index just past the last item belonging to the section starting at `start`, which runs until
/// the next table header.
fn section_end(items: &[RootItem], start: usize) -> usize {
  items[start..]
    .iter()
    .position(RootItem::is_table_header)
    .map(|i| start + i)
    .unwrap_or(items.len())
}

/// The name an entry sorts under. Only the first segment of a dotted key is used, so that
/// `serde.workspace` sorts beside `serde`.
fn entry_sort_key<'a>(entry: &'a Entry<'_>) -> &'a str {
  entry.key.first.text
}

fn sort_cargo_package_section(left: &Entry, right: &Entry) -> Ordering {
  match (entry_sort_key(left), entry_sort_key(right)) {
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

/// Sorts the string members of a `[workspace]` `members` array.
fn sort_workspace_members(entry: &mut Entry) {
  let ValueKind::Array(array) = &mut entry.value.kind else {
    return;
  };
  let all_strings = array.values.iter().all(|value| match &value.value.kind {
    ValueKind::Scalar(text) => text.starts_with('"') || text.starts_with('\''),
    _ => false,
  });
  if !all_strings {
    return;
  }
  let mut units = array
    .values
    .drain(..)
    .map(|mut value| Unit {
      leading_blank: value.leading_comments.first().map(|c| c.blank_line_before).unwrap_or(value.blank_line_before),
      inner_blank: !value.leading_comments.is_empty() && value.blank_line_before,
      starts_group: value.blank_line_before || value.leading_comments.iter().any(|c| c.blank_line_before),
      leading_comments: std::mem::take(&mut value.leading_comments),
      value,
    })
    .collect::<Vec<_>>();
  sort_units(&mut units, |left, right| scalar_text(&left.value).cmp(scalar_text(&right.value)));
  array.values = units
    .into_iter()
    .map(|mut unit| {
      match unit.leading_comments.first_mut() {
        Some(first) => {
          first.blank_line_before = unit.leading_blank;
          unit.value.blank_line_before = unit.inner_blank;
        }
        None => unit.value.blank_line_before = unit.leading_blank,
      }
      ArrayValue {
        leading_comments: unit.leading_comments,
        ..unit.value
      }
    })
    .collect();
}

fn scalar_text<'a>(value: &'a Value<'_>) -> &'a str {
  match &value.kind {
    ValueKind::Scalar(text) => text,
    // the all-strings guard admits nothing else
    _ => "",
  }
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
fn sort_section(items: &mut Vec<RootItem>, start: usize, end: usize, cmp: &impl Fn(&Entry, &Entry) -> Ordering) {
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
