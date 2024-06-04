use eyre::Result;
use notify_debouncer_mini::{DebounceEventResult, DebouncedEventKind};
use winit::event_loop::EventLoopProxy;

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Watcher {
    watcher: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

impl Watcher {
    pub fn new(proxy: EventLoopProxy<PathBuf>) -> Result<Self> {
        let watcher = notify_debouncer_mini::new_debouncer(
            Duration::from_millis(100),
            watch_callback(proxy),
        )?;

        Ok(Self { watcher })
    }

    pub fn unwatch_file(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.watcher.watcher().unwatch(path.as_ref())?;
        Ok(())
    }

    pub fn watch_file(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.watcher
            .watcher()
            .watch(path.as_ref(), notify::RecursiveMode::NonRecursive)?;
        Ok(())
    }
}

fn watch_callback(proxy: EventLoopProxy<PathBuf>) -> impl FnMut(DebounceEventResult) {
    move |event| match event {
        Ok(events) => {
            if let Some(path) = events
                .into_iter()
                .filter(|e| e.kind == DebouncedEventKind::Any)
                .map(|event| event.path)
                .next()
            {
                assert_eq!(
                    path.extension(),
                    Some(OsStr::new("wgsl")),
                    "TODO: Support glsl shaders."
                );

                proxy.send_event(path).expect("Event Loop has been dropped");
            }
        }
        Err(errors) => eprintln!("File watcher error: {errors}"),
    }
}
