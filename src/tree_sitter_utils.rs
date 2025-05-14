use std::collections::VecDeque;

use async_lsp::lsp_types::{Position as LspPosition, Range as LspRange};
use tree_sitter::{Node, Point as TsPoint, Range as TsRange};

/**
    Converts a tree sitter `Point` to an LSP `Position`
*/
#[must_use]
pub const fn ts_point_to_lsp_position(pos: TsPoint) -> LspPosition {
    #[allow(clippy::cast_possible_truncation)]
    LspPosition {
        line: pos.row as u32,
        character: pos.column as u32,
    }
}

/**
    Converts a tree sitter `Range` to an LSP `Range`
*/
#[must_use]
pub const fn ts_range_to_lsp_range(range: TsRange) -> LspRange {
    LspRange {
        start: ts_point_to_lsp_position(range.start_point),
        end: ts_point_to_lsp_position(range.end_point),
    }
}

/**
    Returns `true` if the given tree sitter `Range`
    contains the given LSP `Position`, otherwise `false`

    This is an **inclusive** bounds check, meaning the position is
    considered *inside* even if it lies on a line or column boundary
*/
#[must_use]
pub const fn ts_range_contains_lsp_position(range: TsRange, pos: LspPosition) -> bool {
    let point = lsp_position_to_ts_point(pos);
    point.row >= range.start_point.row
        && point.column >= range.start_point.column
        && point.row <= range.end_point.row
        && point.column <= range.end_point.column
}

/**
    Returns `true` if the given tree sitter `Range`
    contains the given tree sitter `Point`, otherwise `false`

    This is an **inclusive** bounds check, meaning the point is
    considered *inside* even if it lies on a line or column boundary
*/
#[must_use]
pub const fn ts_range_contains_ts_point(range: TsRange, point: TsPoint) -> bool {
    point.row >= range.start_point.row
        && point.column >= range.start_point.column
        && point.row <= range.end_point.row
        && point.column <= range.end_point.column
}

/**
    Converts an LSP `Position` to a tree sitter `Point`
*/
#[must_use]
pub const fn lsp_position_to_ts_point(pos: LspPosition) -> TsPoint {
    TsPoint {
        row: pos.line as usize,
        column: pos.character as usize,
    }
}

/**
    Finds the first child node that matches the given predicate.
*/
#[must_use]
pub fn find_child<'a, F>(node: Node<'a>, predicate: F) -> Option<Node<'a>>
where
    F: Fn(Node<'a>) -> bool,
{
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|child| predicate(*child))
}

/**
    Finds the first ancestor node that matches the given predicate.
*/
#[must_use]
pub fn find_ancestor<'a, F>(node: Node<'a>, predicate: F) -> Option<Node<'a>>
where
    F: Fn(Node<'a>) -> bool,
{
    let mut current = node.parent();

    while let Some(node) = current {
        if predicate(node) {
            return Some(node);
        }
        current = node.parent();
    }

    None
}

/**
    Finds the first descendant node that matches the given predicate.

    This will search descendants in a depth-first manner.
*/
#[must_use]
pub fn find_descendant<'a, F>(node: Node<'a>, predicate: F) -> Option<Node<'a>>
where
    F: Fn(Node<'a>) -> bool,
{
    let mut cursor = node.walk();
    let mut stack = VecDeque::from([node]);

    while let Some(current) = stack.pop_front() {
        if predicate(current) {
            return Some(current);
        }
        for child in current.children(&mut cursor) {
            stack.push_back(child);
        }
    }

    None
}

/**
    Finds the nearest node at `pos` that also matches the given predicate

    1. If the given node itself matches the predicate, returns the node
    2. If the given node has a child that matches the predicate, returns the child
    3. If the given node has a descendant that matches the predicate, returns the descendant
    4. If the given node has an ancestor that matches the predicate, returns the ancestor

    Note that this uses **inclusive** bounds checks, meaning that points
    are considered *inside* even if they lie on a line or column boundary
*/
pub fn find_nearest<'a, F>(node: Node<'a>, pos: LspPosition, predicate: F) -> Option<Node<'a>>
where
    F: Fn(Node<'a>) -> bool,
{
    // Make sure that we are actually inside this node, first of all ...
    if ts_range_contains_lsp_position(node.range(), pos) {
        // We are inside the node, check it
        if predicate(node) {
            return Some(node);
        }
        // Node is not of kind, check children + descendants + ancestors
        // This may do some redundant work for descendants, but unfortunately
        // there is no easy way to find node depth and skip the direct children
        find_child(node, |child| {
            ts_range_contains_lsp_position(child.range(), pos) && predicate(child)
        })
        .or_else(|| {
            find_descendant(node, |descendant| {
                ts_range_contains_lsp_position(descendant.range(), pos) && predicate(descendant)
            })
        })
        .or_else(|| find_ancestor(node, &predicate))
    } else {
        // We are not inside the node, but an ancestor may still match the position
        find_ancestor(node, |ancestor| {
            ts_range_contains_lsp_position(ancestor.range(), pos) && predicate(ancestor)
        })
    }
}
