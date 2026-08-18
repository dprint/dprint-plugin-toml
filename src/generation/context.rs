use crate::configuration::Configuration;

pub struct Context<'a> {
  pub config: &'a Configuration,
  /// How many single-line inline tables enclose whatever is being generated. A table within one is
  /// kept on a single line too, since a newline between its braces would not be within a value.
  single_line_table_depth: usize,
  /// How many of those tables hold a multi-line string, whose raw text leaves the printer's idea
  /// of where the line begins out of step with what was written. A width based decision taken
  /// after one on the same line cannot be trusted, so the arrays there are collapsed onto the
  /// table's line instead of being laid out against the line width.
  collapsed_arrays_depth: usize,
}

impl<'a> Context<'a> {
  pub fn new(config: &'a Configuration) -> Self {
    Self {
      config,
      single_line_table_depth: 0,
      collapsed_arrays_depth: 0,
    }
  }

  pub fn is_in_single_line_table(&self) -> bool {
    self.single_line_table_depth > 0
  }

  pub fn are_arrays_collapsed(&self) -> bool {
    self.collapsed_arrays_depth > 0
  }

  pub fn line_context(&self) -> crate::ast::LineContext {
    crate::ast::LineContext {
      within_single_line_table: self.is_in_single_line_table(),
      arrays_collapsed: self.are_arrays_collapsed(),
    }
  }

  pub fn with_single_line_table<T>(&mut self, collapse_arrays: bool, action: impl FnOnce(&mut Self) -> T) -> T {
    self.single_line_table_depth += 1;
    if collapse_arrays {
      self.collapsed_arrays_depth += 1;
    }
    let result = action(self);
    self.single_line_table_depth -= 1;
    if collapse_arrays {
      self.collapsed_arrays_depth -= 1;
    }
    result
  }
}
