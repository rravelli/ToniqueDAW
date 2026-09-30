//! How the window is laid out: which panels are open and what the main
//! area shows. Not part of the project.

/// What the main area shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainView {
    Timeline,
    /// The engine's processing graph, live.
    Graph,
}

pub struct Workspace {
    /// The browser, on the left.
    pub left_panel_open: bool,
    /// The selected track's effects, at the bottom.
    pub bottom_panel_open: bool,
    pub main_view: MainView,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            left_panel_open: true,
            bottom_panel_open: false,
            main_view: MainView::Timeline,
        }
    }
}

impl Workspace {
    /// Between the timeline and the graph.
    pub fn toggle_graph(&mut self) {
        self.main_view = match self.main_view {
            MainView::Graph => MainView::Timeline,
            MainView::Timeline => MainView::Graph,
        };
    }
}
