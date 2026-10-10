//! Undo and redo.
//!
//! A step is a snapshot of the document. Layer images are shared behind `Arc`, so taking a
//! snapshot is cheap and the pixels are only copied when an edit actually writes to them.

use crate::core::document::Document;

/// The default number of steps kept, matching the original's session-only history feel while
/// staying inside a predictable memory budget on a desktop.
pub const DEFAULT_LIMIT: usize = 64;

/// The undo stack.
#[derive(Debug)]
pub struct History {
    past: Vec<(String, Document)>,
    future: Vec<(String, Document)>,
    limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new(DEFAULT_LIMIT)
    }
}

impl History {
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            past: Vec::new(),
            future: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// Records the document as it is **before** an edit, under a label for the Edit menu.
    pub fn record(&mut self, document: &Document, label: impl Into<String>) {
        self.past.push((label.into(), document.clone()));
        if self.past.len() > self.limit {
            self.past.remove(0);
        }
        // A new edit is a new branch: anything that was undone is gone.
        self.future.clear();
    }

    /// Steps back, replacing `document` with the previous state.
    pub fn undo(&mut self, document: &mut Document) -> Option<String> {
        let (label, previous) = self.past.pop()?;
        let current = std::mem::replace(document, previous);
        self.future.push((label.clone(), current));
        Some(label)
    }

    /// Steps forward again.
    pub fn redo(&mut self, document: &mut Document) -> Option<String> {
        let (label, next) = self.future.pop()?;
        let current = std::mem::replace(document, next);
        self.past.push((label.clone(), current));
        Some(label)
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    #[must_use]
    pub fn undo_label(&self) -> Option<&str> {
        self.past.last().map(|(label, _)| label.as_str())
    }

    #[must_use]
    pub fn redo_label(&self) -> Option<&str> {
        self.future.last().map(|(label, _)| label.as_str())
    }

    /// Drops every step, which is what opening a file does.
    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
    }

    #[must_use]
    pub fn depth(&self) -> usize {
        self.past.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pixel::PremulImage;
    use std::sync::Arc;

    fn document_with_one_layer() -> Document {
        let mut document = Document::new(2, 2);
        document.add_layer(
            "Layer",
            Arc::new(PremulImage::filled(2, 2, [255, 0, 0, 255])),
            None,
        );
        document
    }

    #[test]
    fn undo_restores_the_previous_state() {
        let mut document = document_with_one_layer();
        let mut history = History::default();
        history.record(&document, "Add layer");
        document.add_layer(
            "Second",
            Arc::new(PremulImage::filled(2, 2, [0, 255, 0, 255])),
            None,
        );
        assert_eq!(document.layers.len(), 2);
        assert_eq!(history.undo(&mut document).as_deref(), Some("Add layer"));
        assert_eq!(document.layers.len(), 1);
        assert_eq!(history.redo(&mut document).as_deref(), Some("Add layer"));
        assert_eq!(document.layers.len(), 2);
    }

    #[test]
    fn a_new_edit_drops_the_redo_stack() {
        let mut document = document_with_one_layer();
        let mut history = History::default();
        history.record(&document, "First");
        document.dirty = true;
        history.undo(&mut document);
        assert!(history.can_redo());
        history.record(&document, "Second");
        assert!(!history.can_redo());
    }

    #[test]
    fn the_stack_respects_its_limit() {
        let document = document_with_one_layer();
        let mut history = History::new(2);
        history.record(&document, "one");
        history.record(&document, "two");
        history.record(&document, "three");
        assert_eq!(history.depth(), 2);
        assert_eq!(history.undo_label(), Some("three"));
    }

    #[test]
    fn nothing_to_undo_reports_none() {
        let mut document = document_with_one_layer();
        let mut history = History::default();
        assert!(history.undo(&mut document).is_none());
        assert!(!history.can_undo());
    }
}
