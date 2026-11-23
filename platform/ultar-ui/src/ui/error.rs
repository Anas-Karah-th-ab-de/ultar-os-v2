//! The error type of tree operations.

use super::{NodeId, MAX_DEPTH, MAX_NODES};
use crate::icon::PathError;
use crate::text::TextError;
use std::fmt;
use ultar_i18n::ParseError;

/// Why a UI operation was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum UiError {
    /// The text engine could not start.
    Text(TextError),
    /// The icon set could not be parsed.
    Icons(PathError),
    /// A toolkit catalog is malformed.
    Catalog(ParseError),
    /// The node does not exist in this tree.
    UnknownNode(NodeId),
    /// The node cannot have (more) children.
    NotAContainer(NodeId),
    /// The tree already holds [`MAX_NODES`] nodes.
    TooManyNodes,
    /// The tree would be nested deeper than [`MAX_DEPTH`].
    TooDeep,
    /// The node is not the kind of widget the operation needs.
    WrongWidget {
        /// The node.
        node: NodeId,
        /// The widget kind that was expected.
        expected: &'static str,
    },
    /// The widget has no such state (for example loading on a slider).
    NotApplicable(NodeId),
    /// An option index beyond the select's options.
    OptionOutOfRange {
        /// The node.
        node: NodeId,
        /// The index that was asked for.
        index: usize,
    },
    /// Scale factors must be finite and between 0.5 and 8.
    InvalidScale,
    /// Surface sizes must be finite and not negative.
    InvalidSize,
    /// A menu is empty, has more than `MAX_MENU_ITEMS` items in one level or is nested
    /// deeper than `MAX_MENU_DEPTH`.
    InvalidMenu,
}

impl fmt::Display for UiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UiError::Text(e) => write!(f, "text engine: {e}"),
            UiError::Icons(e) => write!(f, "icon set: {e}"),
            UiError::Catalog(e) => write!(f, "toolkit catalog: {e}"),
            UiError::UnknownNode(id) => write!(f, "node {} does not exist", id.0),
            UiError::NotAContainer(id) => write!(f, "node {} cannot take a child", id.0),
            UiError::TooManyNodes => write!(f, "more than {MAX_NODES} nodes"),
            UiError::TooDeep => write!(f, "nesting deeper than {MAX_DEPTH}"),
            UiError::WrongWidget { node, expected } => {
                write!(f, "node {} is not a {expected}", node.0)
            }
            UiError::NotApplicable(id) => write!(f, "node {} has no such state", id.0),
            UiError::OptionOutOfRange { node, index } => {
                write!(f, "node {} has no option {index}", node.0)
            }
            UiError::InvalidScale => write!(f, "scale must be between 0.5 and 8"),
            UiError::InvalidSize => write!(f, "size must be finite and not negative"),
            UiError::InvalidMenu => write!(f, "menu is empty, too long or nested too deep"),
        }
    }
}

impl std::error::Error for UiError {}

impl From<TextError> for UiError {
    fn from(e: TextError) -> UiError {
        UiError::Text(e)
    }
}

impl From<PathError> for UiError {
    fn from(e: PathError) -> UiError {
        UiError::Icons(e)
    }
}

impl From<ParseError> for UiError {
    fn from(e: ParseError) -> UiError {
        UiError::Catalog(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_has_a_message() {
        let node = NodeId(3);
        let errors = [
            UiError::UnknownNode(node),
            UiError::NotAContainer(node),
            UiError::TooManyNodes,
            UiError::TooDeep,
            UiError::WrongWidget {
                node,
                expected: "switch",
            },
            UiError::NotApplicable(node),
            UiError::OptionOutOfRange { node, index: 9 },
            UiError::InvalidScale,
            UiError::InvalidMenu,
            UiError::InvalidSize,
        ];
        for error in errors {
            assert!(!error.to_string().is_empty());
        }
    }
}
