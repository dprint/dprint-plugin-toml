use dprint_core::formatting::conditions::*;
use dprint_core::formatting::*;
use std::cell::Cell;
use std::rc::Rc;
use taplo::rowan::NodeOrToken;
use taplo::syntax::{SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};

use super::Context;
use crate::configuration::Configuration;

type PrintItemsResult = Result<PrintItems, ()>;

pub fn parse_items(node: SyntaxNode, text: &str, config: &Configuration) -> PrintItems {
    let mut context = Context::new(text, config);
    let mut items = parse_node(node.into(), &mut context);
    items.push_condition(if_true(
        "endOfFileNewLine",
        |context| {
            Some(context.writer_info.column_number > 0 || context.writer_info.line_number > 0)
        },
        Signal::NewLine.into(),
    ));
    items
}

fn parse_node<'a>(node: SyntaxElement, context: &mut Context<'a>) -> PrintItems {
    parse_node_with_inner(node, context, |items, _| items)
}

fn parse_node_with_inner<'a>(
    node: SyntaxElement,
    context: &mut Context<'a>,
    inner_parse: impl FnOnce(PrintItems, &mut Context<'a>) -> PrintItems,
) -> PrintItems {
    let mut items = PrintItems::new();
    // println!("{:?}", node);

    if node.kind() != SyntaxKind::COMMENT {
        for comment in get_comments_on_previous_lines(node.clone()) {
            if !context.has_handled_comment(comment.text_range().start().into()) {
                items.extend(parse_comment(comment.clone(), context));
                items.push_signal(Signal::NewLine);
                if NodeOrToken::Token(comment).has_trailing_blank_line() {
                    items.push_signal(Signal::NewLine);
                }
            }
        }
    }

    let result = match node.clone() {
        NodeOrToken::Node(node) => match node.kind() {
            SyntaxKind::ROOT => parse_root(node, context),
            SyntaxKind::ARRAY => parse_array(node, context),
            SyntaxKind::INLINE_TABLE => parse_inline_table(node, context),
            SyntaxKind::ENTRY => parse_entry(node, context),
            SyntaxKind::KEY => parse_key(node, context),
            SyntaxKind::VALUE => parse_value(node, context),
            SyntaxKind::TABLE_HEADER => parse_table_header(node, context),
            SyntaxKind::TABLE_ARRAY_HEADER => parse_table_array_header(node, context),
            _ => Err(()),
        },
        NodeOrToken::Token(token) => match token.kind() {
            SyntaxKind::COMMENT => Ok(parse_comment(token, context)),
            _ => Ok(parser_helpers::parse_string(token.text().trim().into())),
        },
    };

    items.extend(inner_parse(
        match result {
            Ok(items) => items,
            Err(()) => parser_helpers::parse_raw_string_trim_line_ends(node.text().trim().into()),
        },
        context,
    ));

    if matches!(
        node.kind(),
        SyntaxKind::VALUE | SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ARRAY_HEADER
    ) {
        for comment in node.child_comments() {
            items.extend(parse_comment(comment, context));
        }
    }
    if node.parent().is_none()
        || node.parent().unwrap().kind() != SyntaxKind::VALUE
        || !node.is_last_non_trivia_sibling()
    {
        if let Some(trailing_comment) = get_trailing_comment(node) {
            items.extend(parse_comment(trailing_comment, context));
        }
    }

    items
}

fn parse_root<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
    // print_formatted_tree(node.clone());

    let mut found_first = false;
    let new_line_count = Rc::new(Cell::new(0));
    let mut parse_element = {
        let new_line_count = new_line_count.clone();
        move |element: SyntaxElement| {
            let mut items = PrintItems::new();
            if found_first {
                items.push_signal(Signal::NewLine);
                if new_line_count.get() > 1 {
                    items.push_signal(Signal::NewLine);
                }
            }
            items.extend(parse_node(element, context));

            found_first = true;
            new_line_count.set(0);
            items
        }
    };

    let mut items = PrintItems::new();
    for element in node.children_with_tokens() {
        match element {
            NodeOrToken::Node(_) => items.extend(parse_element(element)),
            NodeOrToken::Token(_) => match element.kind() {
                SyntaxKind::NEWLINE => {
                    new_line_count.set(new_line_count.get() + element.text().chars().count())
                }
                SyntaxKind::COMMENT => items.extend(parse_element(element)),
                _ => {}
            },
        }
    }

    Ok(items)
}

fn parse_array<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
    let values = node.children();
    let open_token = get_token_with_kind(node.clone(), SyntaxKind::BRACKET_START)?;
    let close_token = get_token_with_kind(node.clone(), SyntaxKind::BRACKET_END)?;
    let is_in_inline_table = node
        .ancestors()
        .any(|a| a.kind() == SyntaxKind::INLINE_TABLE);
    let force_use_new_lines = !is_in_inline_table && has_following_newline(open_token.clone());
    ensure_all_kind(values.clone(), SyntaxKind::VALUE)?;

    Ok(parse_surrounded_by_tokens(
        |context| {
            parse_comma_separated_values(
                ParseCommaSeparatedValuesOptions {
                    nodes: values.into_iter().map(|v| v.into()).collect::<Vec<_>>(),
                    prefer_hanging: false,
                    force_use_new_lines,
                    allow_blank_lines: true,
                    single_line_space_at_start: false,
                    single_line_space_at_end: false,
                    custom_single_line_separator: None,
                    multi_line_options:
                        parser_helpers::MultiLineOptions::surround_newlines_indented(),
                    force_possible_newline_at_start: false,
                },
                context,
            )
        },
        ParseSurroundedByTokensParams {
            open_token,
            close_token,
        },
        context,
    ))
}

fn parse_inline_table<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
    let values = node.children();
    ensure_all_kind(values.clone(), SyntaxKind::ENTRY)?;

    let mut items = PrintItems::new();
    items.push_str("{");
    let mut had_item = false;
    for (i, value) in values.enumerate() {
        items.push_str(if i > 0 { ", " } else { " " });
        items.extend(parse_node(value.into(), context));
        had_item = true;
    }
    items.push_str(if had_item { " }" } else { "}" });

    // the comment seems to be stored as the last child of an inline table, so check for it here
    if let Some(NodeOrToken::Token(token)) = node.children_with_tokens().last() {
        if token.kind() == SyntaxKind::COMMENT {
            items.extend(parse_comment(token.into(), context));
        }
    }

    // Disable newlines in a table. The spec says the following:
    // > Inline tables are intended to appear on a single line. A terminating comma (also called trailing comma)
    // > is not permitted after the last key/value pair in an inline table. No newlines are allowed between the
    // > curly braces unless they are valid within a value. Even so, it is strongly discouraged to break an inline
    // > table onto multiples lines. If you find yourself gripped with this desire, it means you should be using
    // > standard tables.
    Ok(parser_helpers::with_no_new_lines(items))
}

fn parse_entry<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
    let key = get_child_with_kind(node.clone(), SyntaxKind::KEY)?;
    let value = get_child_with_kind(node.clone(), SyntaxKind::VALUE)?;
    let mut items = PrintItems::new();

    items.extend(parse_node(key.into(), context));
    items.push_str(" = ");
    items.extend(parse_node(value.into(), context));

    Ok(items)
}

fn parse_key<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
    // Spec: A key may be either bare, quoted, or dotted.
    Ok(parse_children_inline(node, context))
}

fn parse_value<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
    // Spec: Values must be either String, Integer, Float, Boolean, DateTimes, Array, InlineTable
    Ok(parse_children_inline(node, context))
}

fn parse_children_inline<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    for element in get_children_with_non_trivia_tokens(node) {
        items.extend(parse_node(element, context));
    }
    items
}

fn parse_table_header<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
    // Spec: Naming rules for tables are the same as for keys
    let key = get_child_with_kind(node.clone(), SyntaxKind::KEY)?;
    let mut items = PrintItems::new();
    items.push_str("[");
    items.extend(parse_node(key.into(), context));
    items.push_str("]");
    Ok(items)
}

fn parse_table_array_header<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
    // Spec: Naming rules for tables are the same as for keys
    let key = get_child_with_kind(node.clone(), SyntaxKind::KEY)?;
    let mut items = PrintItems::new();
    items.push_str("[[");
    items.extend(parse_node(key.into(), context));
    items.push_str("]]");
    Ok(items)
}

struct ParseSurroundedByTokensParams {
    open_token: SyntaxToken,
    close_token: SyntaxToken,
}

fn parse_surrounded_by_tokens<'a, 'b>(
    parse_inner: impl FnOnce(&mut Context<'a>) -> PrintItems,
    opts: ParseSurroundedByTokensParams,
    context: &mut Context<'a>,
) -> PrintItems {
    // parse
    let mut items = PrintItems::new();
    items.extend(parse_node(opts.open_token.clone().into(), context));

    items.extend(parse_inner(context));

    let before_trailing_comments_info = Info::new("beforeTrailingComments");
    items.push_info(before_trailing_comments_info);

    for comment in get_comments_on_previous_lines(opts.close_token.clone().into()) {
        if NodeOrToken::Token(comment.clone()).has_leading_blank_line() {
            items.push_signal(Signal::NewLine);
        }
        items.extend(parser_helpers::with_indent(parse_comment(comment, context)));
        items.push_signal(Signal::NewLine);
    }

    items.extend(parse_node(opts.close_token.into(), context));
    items
}

struct ParseCommaSeparatedValuesOptions {
    nodes: Vec<SyntaxElement>,
    prefer_hanging: bool,
    force_use_new_lines: bool,
    allow_blank_lines: bool,
    single_line_space_at_start: bool,
    single_line_space_at_end: bool,
    custom_single_line_separator: Option<PrintItems>,
    multi_line_options: parser_helpers::MultiLineOptions,
    force_possible_newline_at_start: bool,
}

fn parse_comma_separated_values<'a>(
    opts: ParseCommaSeparatedValuesOptions,
    context: &mut Context<'a>,
) -> PrintItems {
    let nodes = opts.nodes;
    let indent_width = context.config.indent_width;
    let compute_lines_span = opts.allow_blank_lines; // save time otherwise
    parser_helpers::parse_separated_values(
        |is_multi_line_ref| {
            let mut parsed_nodes = Vec::new();
            let nodes_count = nodes.len();
            for (i, value) in nodes.into_iter().enumerate() {
                let (allow_inline_multi_line, allow_inline_single_line) =
                    (value.kind() == SyntaxKind::INLINE_TABLE, false);
                let lines_span = if compute_lines_span {
                    Some(parser_helpers::LinesSpan {
                        start_line: context
                            .get_line_number_at_pos(value.start_including_leading_comments()),
                        end_line: context.get_line_number_at_pos(value.text_range().end().into()),
                    })
                } else {
                    None
                };
                let items = parser_helpers::new_line_group({
                    let parsed_comma = if i == nodes_count - 1 {
                        // todo: make this conditional based on config
                        let is_multi_line = is_multi_line_ref.create_resolver();
                        if_true("commaIfMultiLine", is_multi_line, ",".into()).into()
                    } else {
                        ",".into()
                    };
                    parse_comma_separated_value(value, parsed_comma, context)
                });
                parsed_nodes.push(parser_helpers::ParsedValue {
                    items,
                    lines_span,
                    allow_inline_multi_line,
                    allow_inline_single_line,
                });
            }

            parsed_nodes
        },
        parser_helpers::ParseSeparatedValuesOptions {
            prefer_hanging: opts.prefer_hanging,
            force_use_new_lines: opts.force_use_new_lines,
            allow_blank_lines: opts.allow_blank_lines,
            single_line_space_at_start: opts.single_line_space_at_start,
            single_line_space_at_end: opts.single_line_space_at_end,
            single_line_separator: opts
                .custom_single_line_separator
                .unwrap_or(Signal::SpaceOrNewLine.into()),
            indent_width,
            multi_line_options: opts.multi_line_options,
            force_possible_newline_at_start: opts.force_possible_newline_at_start,
        },
    )
    .items
}

fn parse_comma_separated_value<'a>(
    value: SyntaxElement,
    parsed_comma: PrintItems,
    context: &mut Context<'a>,
) -> PrintItems {
    let mut items = PrintItems::new();
    let comma_token = get_next_comma_sibling(value.clone());

    let parsed_comma = parsed_comma.into_rc_path();
    items.extend(parse_node_with_inner(
        value,
        context,
        move |mut items, _| {
            // this Rc clone is necessary because we can't move the captured parsed_comma out of this closure
            items.push_optional_path(parsed_comma.clone());
            items
        },
    ));

    // get the trailing comments after the comma token
    if let Some(comma_token) = comma_token {
        items.extend(parse_trailing_comment(comma_token.into(), context));
    }

    items
}

fn parse_trailing_comment<'a>(element: SyntaxElement, context: &mut Context<'a>) -> PrintItems {
    match get_trailing_comment(element) {
        Some(comment) => parse_comment(comment, context),
        None => PrintItems::new(),
    }
}

fn parse_comment<'a>(comment: SyntaxToken, context: &mut Context<'a>) -> PrintItems {
    let pos = comment.text_range().start().into();
    if context.has_handled_comment(pos) {
        return PrintItems::new();
    }
    context.add_handled_comment(pos);

    #[cfg(debug_assertions)]
    debug_assert_kind(comment.clone().into(), SyntaxKind::COMMENT);

    let mut items = PrintItems::new();
    items.push_condition(if_false(
        "spaceIfNotStartOfLine",
        |context| Some(condition_resolvers::is_start_of_line(context)),
        " ".into(),
    ));
    items.extend(parser_helpers::parse_raw_string(comment.text()));
    items.push_signal(Signal::ExpectNewLine);
    items
}

#[allow(dead_code)]
#[cfg(debug_assertions)]
fn print_formatted_tree(node: SyntaxNode) {
    print_node_and_children(node, 0);

    fn print_node_and_children(node: SyntaxNode, indent: usize) {
        println!("{}{:?}", " ".repeat(indent), node);
        for c in node.children_with_tokens() {
            match c {
                NodeOrToken::Node(c) => {
                    print_node_and_children(c.clone(), indent + 2);
                }
                NodeOrToken::Token(t) => {
                    println!("{}{:?} [TOKEN]", " ".repeat(indent + 2), t);
                }
            }
        }
    }
}

fn ensure_all_kind(nodes: impl Iterator<Item = SyntaxNode>, kind: SyntaxKind) -> Result<(), ()> {
    for node in nodes {
        ensure_kind(node.into(), kind)?;
    }
    Ok(())
}

fn ensure_kind(element: SyntaxElement, kind: SyntaxKind) -> Result<(), ()> {
    if element.kind() == kind {
        Ok(())
    } else {
        Err(())
    }
}

#[cfg(debug_assertions)]
fn debug_assert_kind(element: SyntaxElement, kind: SyntaxKind) {
    if element.kind() != kind {
        panic!(
            "Debug Assertion: Expected kind {:?}, but was {:?}",
            kind,
            element.kind()
        );
    }
}

fn has_following_newline(token: SyntaxToken) -> bool {
    let mut element: SyntaxElement = token.into();
    while let Some(sibling) = element.next_sibling_or_token() {
        element = sibling.clone();
        match sibling {
            NodeOrToken::Token(token) => match token.kind() {
                SyntaxKind::WHITESPACE => continue,
                SyntaxKind::NEWLINE | SyntaxKind::COMMENT => return true,
                _ => break,
            },
            NodeOrToken::Node(_) => break,
        }
    }
    false
}

fn get_next_comma_sibling(mut element: SyntaxElement) -> Option<SyntaxToken> {
    while let Some(sibling) = element.next_sibling_or_token() {
        element = sibling.clone();
        match sibling {
            NodeOrToken::Token(token) => match token.kind() {
                SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT => continue,
                SyntaxKind::COMMA => return Some(token),
                _ => break,
            },
            NodeOrToken::Node(_) => break,
        }
    }
    None
}

fn get_trailing_comment(mut element: SyntaxElement) -> Option<SyntaxToken> {
    while let Some(sibling) = element.next_sibling_or_token() {
        element = sibling.clone();
        match sibling {
            NodeOrToken::Token(token) => match token.kind() {
                SyntaxKind::WHITESPACE => continue,
                SyntaxKind::COMMENT => return Some(token),
                _ => break,
            },
            NodeOrToken::Node(_) => break,
        }
    }
    None
}

fn get_comments_on_previous_lines(mut element: SyntaxElement) -> Vec<SyntaxToken> {
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

fn get_children_with_non_trivia_tokens(node: SyntaxNode) -> impl Iterator<Item = SyntaxElement> {
    node.children_with_tokens().filter_map(|c| match c {
        NodeOrToken::Token(token) => {
            if token.kind() != SyntaxKind::WHITESPACE && token.kind() != SyntaxKind::COMMENT {
                Some(token.into())
            } else {
                None
            }
        }
        NodeOrToken::Node(node) => Some(node.into()),
    })
}

fn get_child_with_kind(node: SyntaxNode, kind: SyntaxKind) -> Result<SyntaxNode, ()> {
    match node.children().find(|c| c.kind() == kind) {
        Some(node) => Ok(node),
        None => Err(()),
    }
}

fn get_token_with_kind(node: SyntaxNode, kind: SyntaxKind) -> Result<SyntaxToken, ()> {
    match node.children_with_tokens().find(|c| c.kind() == kind) {
        Some(NodeOrToken::Token(token)) => Ok(token),
        _ => Err(()),
    }
}

pub trait SyntaxElementExtensions {
    fn text(&self) -> String;
    fn start_including_leading_comments(&self) -> usize;
    fn child_comments(&self) -> Vec<SyntaxToken>;
    fn is_last_non_trivia_sibling(&self) -> bool;
    fn has_leading_blank_line(&self) -> bool;
    fn has_trailing_blank_line(&self) -> bool;
}

impl SyntaxElementExtensions for SyntaxElement {
    fn text(&self) -> String {
        match self {
            NodeOrToken::Node(node) => node.text().to_string(),
            NodeOrToken::Token(token) => token.text().to_string(),
        }
    }

    fn start_including_leading_comments(&self) -> usize {
        let result = get_comments_on_previous_lines(self.clone());
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
}
