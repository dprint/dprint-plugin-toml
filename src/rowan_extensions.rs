use taplo::{
    rowan::NodeOrToken,
    syntax::{SyntaxElement, SyntaxKind, SyntaxToken},
};

pub trait SyntaxElementExtensions {
    fn text(&self) -> String;
    fn start_including_leading_comments(&self) -> usize;
    fn child_comments(&self) -> Vec<SyntaxToken>;
    fn is_last_non_trivia_sibling(&self) -> bool;
    fn has_leading_blank_line(&self) -> bool;
    fn has_trailing_blank_line(&self) -> bool;
    fn get_comments_on_previous_lines(&self) -> Vec<SyntaxToken>;
}

impl SyntaxElementExtensions for SyntaxElement {
    fn text(&self) -> String {
        match self {
            NodeOrToken::Node(node) => node.text().to_string(),
            NodeOrToken::Token(token) => token.text().to_string(),
        }
    }

    fn start_including_leading_comments(&self) -> usize {
        let result = self.get_comments_on_previous_lines();
        if let Some(comment) = result.get(0) {
            comment.text_range().start().into()
        } else {
            self.text_range().start().into()
        }
    }

    fn child_comments(&self) -> Vec<SyntaxToken> {
        match self {
            NodeOrToken::Token(_) => Vec::with_capacity(0),
            NodeOrToken::Node(node) => node
                .children_with_tokens()
                .filter_map(|c| match c {
                    NodeOrToken::Token(token) if token.kind() == SyntaxKind::COMMENT => Some(token),
                    _ => None,
                })
                .collect(),
        }
    }

    fn is_last_non_trivia_sibling(&self) -> bool {
        let mut element = self.clone();
        while let Some(sibling) = element.next_sibling_or_token() {
            element = sibling.clone();
            match sibling {
                NodeOrToken::Token(token) => match token.kind() {
                    SyntaxKind::WHITESPACE => continue,
                    SyntaxKind::NEWLINE => continue,
                    SyntaxKind::COMMENT => continue,
                    _ => return false,
                },
                NodeOrToken::Node(_) => return false,
            }
        }

        true
    }

    fn has_leading_blank_line(&self) -> bool {
        let mut element = self.clone();
        let mut found_new_line = false;
        while let Some(sibling) = element.prev_sibling_or_token() {
            element = sibling.clone();
            match sibling {
                NodeOrToken::Token(token) => match token.kind() {
                    SyntaxKind::WHITESPACE => continue,
                    SyntaxKind::NEWLINE => {
                        if found_new_line || token.text().chars().count() > 1 {
                            return true;
                        } else {
                            found_new_line = true;
                        }
                    }
                    _ => return false,
                },
                NodeOrToken::Node(_) => return false,
            }
        }

        false
    }

    fn has_trailing_blank_line(&self) -> bool {
        let mut element = self.clone();
        let mut found_new_line = false;
        while let Some(sibling) = element.next_sibling_or_token() {
            element = sibling.clone();
            match sibling {
                NodeOrToken::Token(token) => match token.kind() {
                    SyntaxKind::WHITESPACE => continue,
                    SyntaxKind::NEWLINE => {
                        if found_new_line || token.text().chars().count() > 1 {
                            return true;
                        } else {
                            found_new_line = true;
                        }
                    }
                    _ => return false,
                },
                NodeOrToken::Node(_) => return false,
            }
        }

        false
    }

    fn get_comments_on_previous_lines(&self) -> Vec<SyntaxToken> {
        let mut element = self.clone();
        let mut comments = Vec::new();
        let mut pending_comment = None;
        while let Some(sibling) = element.prev_sibling_or_token() {
            element = sibling.clone();
            match sibling {
                NodeOrToken::Token(token) => match token.kind() {
                    SyntaxKind::WHITESPACE => continue,
                    SyntaxKind::NEWLINE => {
                        if let Some(comment) = pending_comment.take() {
                            comments.push(comment);
                        }
                    }
                    SyntaxKind::COMMENT => {
                        pending_comment.replace(token);
                    }
                    _ => break,
                },
                NodeOrToken::Node(_) => break,
            }
        }
        comments.reverse();
        comments
    }
}
