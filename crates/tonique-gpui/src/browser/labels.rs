//! Colour labels on files and folders, kept in Tonique's config: shown as
//! dots in both lists, and as a section each in the labels' list. Given
//! from a row's menu, or with 1 to 7 (0 takes one off).

use super::BrowserPanel;
use std::path::{Path, PathBuf};

use gpui::Context;

use crate::{
    browser::{LabelView, ListKind},
    theme::Color,
};
use tonique_core::utils::color::parse_color;

/// Shown for a label whose colour can't be read.
const FALLBACK: Color = Color::rgb(128, 128, 128);

/// The path naming label `index`'s section in the labels' list: no file's.
fn section_path(index: usize) -> PathBuf {
    PathBuf::from(format!("\u{0}label-{index}"))
}

impl BrowserPanel {
    /// `path`'s label, if it has one.
    pub(super) fn label_of(&self, path: &Path) -> Option<&LabelView> {
        self.labels.get(*self.item_labels.get(path)?)
    }

    /// Show the labels as configured, in both lists.
    pub(super) fn refresh_labels(&mut self) {
        let labels = self.config.labels();
        self.labels = labels
            .all()
            .iter()
            .map(|label| LabelView {
                name: label.name.clone(),
                color: parse_color(&label.color).unwrap_or(FALLBACK),
            })
            .collect();
        self.item_labels = labels
            .assigned()
            .map(|(path, label)| (path.clone(), label))
            .collect();
        let sections: Vec<(PathBuf, Vec<PathBuf>)> = (0..self.labels.len())
            .map(|index| (section_path(index), labels.items(index)))
            .collect();
        self.sections = sections
            .iter()
            .enumerate()
            .map(|(index, (header, items))| (header.clone(), (index, items.len())))
            .collect();
        self.labelled.selected = None;
        self.labelled
            .tree
            .set_sections(sections, &self.labels_collapsed);
    }
}

impl BrowserPanel {
    /// The label of the section row `index` of `kind` heads, and how many
    /// items it has: none for other rows.
    pub(super) fn row_section(&self, kind: ListKind, index: usize) -> Option<(usize, usize)> {
        let item = self.list(kind).tree.items.get(index)?;
        self.sections.get(&item.path).copied()
    }

    /// Change the labels in the config, and show them.
    fn edit_labels(
        &mut self,
        edit: impl FnOnce(&mut tonique_core::config::labels::Labels),
        context: &mut Context<Self>,
    ) {
        self.config.edit_labels(edit);
        self.refresh_labels();
        self.search_again(ListKind::Labels, context);
    }

    /// Give `path` `label`, or (`None`) take its label off.
    pub(super) fn set_label(
        &mut self,
        path: PathBuf,
        label: Option<usize>,
        context: &mut Context<Self>,
    ) {
        self.edit_labels(|labels| labels.set(path, label), context);
    }

    pub(super) fn rename_label(&mut self, index: usize, name: String, context: &mut Context<Self>) {
        self.edit_labels(|labels| labels.rename(index, name), context);
    }

    /// To `color`, one of [`tonique_core::config::labels::COLORS`].
    pub(super) fn recolor_label(&mut self, index: usize, color: &str, context: &mut Context<Self>) {
        let color = color.to_string();
        self.edit_labels(|labels| labels.recolor(index, color), context);
    }

    /// Take label `index` off everything.
    pub(super) fn clear_label(&mut self, index: usize, context: &mut Context<Self>) {
        self.edit_labels(|labels| labels.clear(index), context);
    }

    /// `digit` pressed in `kind`'s list: 1 to 7 give the row selected that
    /// label (or take it off, it being its own), 0 takes its label off.
    /// Not a label's header. Returns whether it was taken.
    pub(super) fn label_key(
        &mut self,
        kind: ListKind,
        digit: usize,
        context: &mut Context<Self>,
    ) -> bool {
        let browser = &*self;
        let Some(path) = browser.list(kind).selected_path().map(Path::to_path_buf) else {
            return false;
        };
        if browser.sections.contains_key(&path) {
            return false;
        }
        let label = digit
            .checked_sub(1)
            .filter(|&label| label < browser.labels.len());
        if digit > 0 && label.is_none() {
            return false;
        }
        let label = label.filter(|&label| browser.item_labels.get(&path) != Some(&label));
        self.set_label(path, label, context);
        true
    }
}
