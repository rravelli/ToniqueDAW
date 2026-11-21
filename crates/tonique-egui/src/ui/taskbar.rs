//! Progress on the app's taskbar entry, e.g. while exporting.
//!
//! Linux only for now, through the Unity LauncherEntry D-Bus signal: KDE
//! Plasma's task manager, Dash to Dock and Plank draw it over the icon. The
//! taskbar matches it to the window through `Tonique.desktop`
//! (`packaging/linux`), which must be installed.

pub struct TaskbarProgress {
    /// What the taskbar shows, in percent: only changes are sent.
    shown: Option<u8>,
    #[cfg(target_os = "linux")]
    launcher: Option<linux::Launcher>,
}

impl TaskbarProgress {
    pub fn new() -> Self {
        Self {
            shown: None,
            #[cfg(target_os = "linux")]
            launcher: None,
        }
    }

    /// Show `progress` (0 to 1) on the icon, or hide the bar with `None`.
    pub fn set(&mut self, progress: Option<f32>) {
        let percent = progress.map(|p| (p.clamp(0., 1.) * 100.).round() as u8);
        if percent == self.shown {
            return;
        }
        self.shown = percent;
        #[cfg(target_os = "linux")]
        self.launcher
            .get_or_insert_with(linux::Launcher::start)
            .send(percent);
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        collections::HashMap,
        sync::mpsc::{Sender, channel},
        thread,
    };

    use zbus::{blocking::Connection, names::BusName, zvariant::Value};

    /// The desktop file the taskbar knows the app by: the window's app id.
    const APP_URI: &str = "application://Tonique.desktop";
    const PATH: &str = "/com/bytenosis/Tonique";
    const INTERFACE: &str = "com.canonical.Unity.LauncherEntry";

    /// Sends the progress from its own thread: connecting to the bus must not
    /// hold up the UI.
    pub struct Launcher(Sender<Option<u8>>);

    impl Launcher {
        pub fn start() -> Self {
            let (sender, receiver) = channel::<Option<u8>>();
            let spawned = thread::Builder::new()
                .name("taskbar".into())
                .spawn(move || {
                    let bus = match Connection::session() {
                        Ok(bus) => bus,
                        Err(e) => {
                            eprintln!("No taskbar progress, couldn't reach D-Bus: {e}");
                            return;
                        }
                    };
                    // Ends with the app, when the sender is dropped
                    for percent in receiver {
                        if let Err(e) = update(&bus, percent) {
                            eprintln!("Couldn't update the taskbar progress: {e}");
                        }
                    }
                });
            if let Err(e) = spawned {
                eprintln!("No taskbar progress: {e}");
            }
            Self(sender)
        }

        pub fn send(&self, percent: Option<u8>) {
            // Fails only if the thread gave up, which it already reported
            let _ = self.0.send(percent);
        }
    }

    fn update(bus: &Connection, percent: Option<u8>) -> zbus::Result<()> {
        let progress = f64::from(percent.unwrap_or(0)) / 100.;
        let properties = HashMap::from([
            ("progress", Value::from(progress)),
            ("progress-visible", Value::from(percent.is_some())),
        ]);
        bus.emit_signal(
            None::<BusName>,
            PATH,
            INTERFACE,
            "Update",
            &(APP_URI, properties),
        )
    }
}
