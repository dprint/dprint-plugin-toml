use std::collections::HashSet;
use crate::configuration::Configuration;

pub struct Context<'a> {
    pub config: &'a Configuration,
    pub text: &'a str,
    handled_comments: HashSet<usize>,
}

impl<'a> Context<'a> {
    pub fn new(text: &'a str, config: &'a Configuration) -> Self {
        Self {
            config,
            text,
            handled_comments: HashSet::new(),
        }
    }

    pub fn has_handled_comment(&self, pos: usize) -> bool {
        self.handled_comments.contains(&pos)
    }

    pub fn add_handled_comment(&mut self, pos: usize) {
        self.handled_comments.insert(pos);
    }

    pub fn get_line_number_at_pos(&self, pos: usize) -> usize {
        // todo: make this faster by using an array of line indexes
        let mut line_number = 0;
        for (i, c) in self.text.char_indices() {
            if pos <= i {
                break;
            }
            if c == '\n' {
                line_number += 1;
            }
        }
        line_number
    }
}
