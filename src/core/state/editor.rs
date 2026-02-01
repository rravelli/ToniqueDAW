pub struct EditorState {
    pub resized_clip: Option<(String, f32, f32, f32)>,
    pub left_panel_open: bool,
    pub bottom_panel_open: bool,
    pub show_export: bool,
}

impl EditorState {
    pub fn new() -> Self {
        Self {
            left_panel_open: true,
            bottom_panel_open: false,
            resized_clip: None,
            show_export: false,
        }
    }
}
