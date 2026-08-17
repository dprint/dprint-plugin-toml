use std::cmp::Ordering;
use std::path::Path;

use crate::ast::*;
use crate::sorting::section_end;
use crate::sorting::sort_root_entries;
use crate::sorting::sort_with_comments;

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
          Section::Package => sort_root_entries(&mut root.items, index + 1, end, &sort_cargo_package_section),
          Section::Dependencies => sort_root_entries(&mut root.items, index + 1, end, &|left, right| entry_sort_key(left).cmp(entry_sort_key(right))),
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

/// The name an entry sorts under. Only the first segment of a dotted key is used, so that
/// `serde.workspace` sorts beside `serde`.
fn entry_sort_key<'a>(entry: &'a Entry<'_>) -> &'a str {
  entry.key.first.text
}

fn sort_cargo_package_section(left: &Entry, right: &Entry) -> Ordering {
  match (entry_sort_key(left), entry_sort_key(right)) {
    // the ranked arms below would otherwise answer the same way in both directions for two entries
    // sharing a key, which is not an ordering and leaves `sort_by` free to do anything
    (left, right) if left == right => Ordering::Equal,
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
  sort_with_comments(&mut array.values, |left, right| scalar_text(&left.value).cmp(scalar_text(&right.value)));
}

fn scalar_text<'a>(value: &'a Value<'_>) -> &'a str {
  match &value.kind {
    ValueKind::Scalar(text) => text,
    // the all-strings guard admits nothing else
    _ => "",
  }
}
