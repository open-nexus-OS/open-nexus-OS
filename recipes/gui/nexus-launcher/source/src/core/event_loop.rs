// src/core/event_loop.rs
// Minimal polling loop that drives a set of UI components and a monotonic timer.
// NOTE: This is intentionally simple; your UI components can still read
//       their own window.events() within on_tick() until you migrate fully.

use event::{EventQueue, EventFlags, user_data};
use libredox::data::TimeSpec;
use std::fs::File;
use std::io::{Read, Write};
use std::mem;
use std::os::unix::io::AsRawFd;

/// Basic trait you can implement for UI building blocks (ActionBar, Taskbar, StartMenu).
pub trait UiComponent {
    /// Called on every timer tick (e.g. once per second if you arm time/4 that way).
    fn on_tick(&mut self, _dt_ms: u32) {}

    /// Called on screen resize to update layout.
    fn on_layout(&mut self, _w: u32, _h: u32) {}

    /// Return true if a re-render is necessary (dirty flag).
    fn is_dirty(&self) -> bool { true }

    /// Render (if visible/dirty). You can pass your WindowManager in if needed.
    fn render(&mut self) {}
}

/// Drives a list of UiComponents using the monotonic timer.
pub struct EventLoop {
    timer_file: File,
}

impl EventLoop {
    pub fn new() -> std::io::Result<Self> {
        // /scheme/time/4 is a monotonic timer on Redox; adjust path if necessary.
        let mut timer_file = File::open("/scheme/time/4")?;

        // Arm the timer: now + 1 second
        let mut buf = [0u8; core::mem::size_of::<TimeSpec>()];
        match libredox::data::timespec_from_mut_bytes(&mut buf) {
            time => {
                time.tv_sec += 1;
                time.tv_nsec = 0;
            }
        }
        timer_file.write(&buf)?;
        Ok(Self { timer_file })
    }

    pub fn run<C: UiComponent>(&mut self, components: &mut [C]) -> std::io::Result<()> {
        user_data! { enum Ev { Time } }
        let mut q = EventQueue::<Ev>::new().expect("event_loop: failed to create queue");
        q.subscribe(self.timer_file.as_raw_fd() as usize, Ev::Time, EventFlags::READ)?;

        loop {
            let ev = q.next().expect("event_loop: failed next")?;
            if ev.user_data == Ev::Time {
                // Read + re-arm timer
                let mut buf = [0u8; mem::size_of::<TimeSpec>()];
                if self.timer_file.read(&mut buf)? < mem::size_of::<TimeSpec>() {
                    continue;
                }
                // Notify components
                for c in components.iter_mut() {
                    c.on_tick(1000);
                    if c.is_dirty() { c.render(); }
                }

                // Re-arm for next second
                match libredox::data::timespec_from_mut_bytes(&mut buf) {
                    time => {
                        time.tv_sec += 1;
                        time.tv_nsec = 0;
                    }
                }
                self.timer_file.write(&buf)?;
            }
        }
    }
}
