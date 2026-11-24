//! Where everything in a header goes, as Tonique's `HeaderLayout`: the
//! colour bar on the left (indented by the groups the row is in), the meter down the right edge, between them a
//! first line (caret, name, mute and solo), and the volume and pan below
//! it when the row is tall enough.

use crate::geometry::Rect;

const PADDING: f32 = 4.;
pub const BUTTON_WIDTH: f32 = 18.;
pub const BUTTON_HEIGHT: f32 = 16.;
const CARET_WIDTH: f32 = 14.;
const METER_WIDTH: f32 = 12.;
const GAP: f32 = 3.;
const LINE_GAP: f32 = 4.;
const SLIDER_HEIGHT: f32 = 16.;
const PAN_WIDTH: f32 = 34.;
pub const COLOR_BAR_WIDTH: f32 = 4.;
/// Indentation per level of groups, up to [`MAX_INDENT_LEVELS`] (deeper
/// rows line up with the deepest shown level).
const INDENT: f32 = 8.;
const MAX_INDENT_LEVELS: usize = 6;
/// Between a header's edges and its content.
pub const HEADER_INSET: f32 = PADDING;

/// Left of the colour bar of a row `depth` groups deep, local to its
/// header. Groups extend theirs down to their last row from there.
pub fn color_bar_left(depth: usize) -> f32 {
    PADDING + depth.min(MAX_INDENT_LEVELS) as f32 * INDENT
}

/// Local to the header.
pub struct HeaderLayout {
    pub bar: Rect,
    pub caret: Rect,
    pub name: Rect,
    pub mute: Rect,
    /// Not on the master.
    pub solo: Option<Rect>,
    /// Volume and pan.
    pub sliders: Option<(Rect, Rect)>,
    pub meter: Rect,
}

impl HeaderLayout {
    /// `depth`: the groups the row is in. `with_solo`: a solo button,
    /// which the master hasn't.
    pub fn new(width: f32, height: f32, depth: usize, collapsed: bool, with_solo: bool) -> Self {
        let inner = Rect::new(
            PADDING,
            PADDING,
            width - 2. * PADDING,
            height - 2. * PADDING,
        );
        let bar = Rect::new(
            color_bar_left(depth),
            inner.top,
            COLOR_BAR_WIDTH,
            inner.height,
        );
        let meter = Rect::new(
            inner.right() - METER_WIDTH,
            inner.top,
            METER_WIDTH,
            inner.height,
        );
        let left = bar.right() + GAP;
        let content = Rect::new(left, inner.top, meter.left - GAP - left, inner.height);
        let has_sliders = !collapsed && content.height >= BUTTON_HEIGHT + LINE_GAP + SLIDER_HEIGHT;
        // Centred in a row too short for the sliders.
        let line_top = if has_sliders {
            content.top
        } else {
            content.top + content.height / 2. - BUTTON_HEIGHT / 2.
        };
        let caret = Rect::new(content.left, line_top, CARET_WIDTH, BUTTON_HEIGHT);
        // From the right: solo, then mute.
        let button =
            |right: f32| Rect::new(right - BUTTON_WIDTH, line_top, BUTTON_WIDTH, BUTTON_HEIGHT);
        let solo = with_solo.then(|| button(content.right()));
        let mute = button(solo.map_or(content.right(), |solo| solo.left - 1.));
        let name_left = caret.right() + 2.;
        let name = Rect::new(
            name_left,
            line_top,
            (mute.left - 1. - GAP - name_left).max(0.),
            BUTTON_HEIGHT,
        );
        let sliders = has_sliders.then(|| {
            let top = line_top + BUTTON_HEIGHT + LINE_GAP;
            let pan = Rect::new(content.right() - PAN_WIDTH, top, PAN_WIDTH, SLIDER_HEIGHT);
            let volume = Rect::new(
                caret.left,
                top,
                (pan.left - GAP - caret.left).max(0.),
                SLIDER_HEIGHT,
            );
            (volume, pan)
        });
        Self {
            bar,
            caret,
            name,
            mute,
            solo,
            sliders,
            meter,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_rows_have_no_sliders_and_centre_their_line() {
        let layout = HeaderLayout::new(190., 30., 0, false, true);
        assert!(layout.sliders.is_none());
        assert_eq!(layout.caret.top + layout.caret.height / 2., 15.);
    }

    #[test]
    fn the_pan_sits_right_of_the_volume() {
        let layout = HeaderLayout::new(190., 60., 0, false, true);
        let (volume, pan) = layout.sliders.unwrap();
        assert!(volume.right() < pan.left);
        assert_eq!(pan.right(), layout.solo.unwrap().right());
        // Collapsed rows never show them.
        assert!(
            HeaderLayout::new(190., 60., 0, true, true)
                .sliders
                .is_none()
        );
    }

    #[test]
    fn without_solo_mute_takes_its_place() {
        let with_solo = HeaderLayout::new(190., 60., 0, false, true);
        let master = HeaderLayout::new(190., 60., 0, false, false);
        assert!(master.solo.is_none());
        assert_eq!(master.mute.right(), with_solo.solo.unwrap().right());
        assert!(master.name.width > with_solo.name.width);
    }

    #[test]
    fn nested_rows_are_indented_up_to_a_limit() {
        let top = HeaderLayout::new(190., 60., 0, false, true);
        let nested = HeaderLayout::new(190., 60., 2, false, true);
        assert!(nested.bar.left > top.bar.left);
        assert!(nested.name.width < top.name.width);
        assert_eq!(color_bar_left(40), color_bar_left(MAX_INDENT_LEVELS));
    }
}
