//! How often gpui-fast draws a near-empty window that asks for every frame.
//! `FULLSCREEN=1` for fullscreen.
use gpui::{
    App, Bounds, Context, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use std::time::Instant;

struct Fps {
    since: Instant,
    frames: u32,
    shown: String,
}

impl Render for Fps {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        window.request_animation_frame();
        self.frames += 1;
        let elapsed = self.since.elapsed().as_secs_f64();
        if elapsed >= 1. {
            self.shown = format!("{:.1} fps", self.frames as f64 / elapsed);
            eprintln!("{}", self.shown);
            self.frames = 0;
            self.since = Instant::now();
        }
        div()
            .size_full()
            .bg(rgb(0x202020))
            .text_color(rgb(0xffffff))
            .child(self.shown.clone())
    }
}

fn main() {
    gpui_platform::application().run(|context: &mut App| {
        let bounds = Bounds::centered(None, size(px(800.), px(600.)), context);
        let full = std::env::var_os("FULLSCREEN").is_some();
        context
            .open_window(
                WindowOptions {
                    low_power_frame_interval: if std::env::var_os("UNCAPPED").is_some() {
                        None
                    } else {
                        Some(std::time::Duration::from_millis(33))
                    },
                    inactive_frame_interval: if std::env::var_os("UNCAPPED").is_some() {
                        None
                    } else {
                        Some(std::time::Duration::from_millis(33))
                    },
                    window_bounds: Some(if full {
                        WindowBounds::Fullscreen(bounds)
                    } else {
                        WindowBounds::Windowed(bounds)
                    }),
                    ..Default::default()
                },
                |_, context| {
                    context.new(|_| Fps {
                        since: Instant::now(),
                        frames: 0,
                        shown: String::new(),
                    })
                },
            )
            .expect("the window opens");
    });
}
