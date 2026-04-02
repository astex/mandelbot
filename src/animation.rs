use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::Color;

use crate::ui::Message;

const FLASH_DURATION: Duration = Duration::from_millis(500);
const FLASH_PEAK: f32 = 0.15;
const FLASH_INTENSITY: f32 = 0.6;
const TICK_INTERVAL: Duration = Duration::from_millis(16);

/// Tracks active flash animations keyed by tab ID.
#[derive(Default)]
pub struct FlashState {
    flashes: HashMap<usize, Instant>,
}

impl FlashState {
    /// Start a flash for the given tab. Returns a tick task if this is the
    /// first active flash (i.e. the tick chain needs to be kicked off).
    pub fn trigger(&mut self, tab_id: usize) -> iced::Task<Message> {
        let was_empty = self.flashes.is_empty();
        self.flashes.insert(tab_id, Instant::now());
        if was_empty { schedule_tick() } else { iced::Task::none() }
    }

    /// Remove expired flashes and schedule the next tick if any remain.
    pub fn tick(&mut self) -> iced::Task<Message> {
        self.flashes.retain(|_, started| started.elapsed() < FLASH_DURATION);
        if self.flashes.is_empty() {
            iced::Task::none()
        } else {
            schedule_tick()
        }
    }

    /// Compute the background color for a tab, blending a flash color over
    /// the base if the tab has an active flash.
    pub fn tab_bg(&self, tab_id: usize, base: Color, flash_color: Color) -> Color {
        let Some(started) = self.flashes.get(&tab_id) else {
            return base;
        };

        let t = envelope(started.elapsed());

        let target = lerp_color(base, flash_color, FLASH_INTENSITY);
        lerp_color(base, target, t)
    }
}

/// Fast fade-in, quadratic ease-out envelope — like a camera flash.
fn envelope(elapsed: Duration) -> f32 {
    let progress = (elapsed.as_secs_f32() / FLASH_DURATION.as_secs_f32()).min(1.0);
    if progress < FLASH_PEAK {
        progress / FLASH_PEAK
    } else {
        let fade = (progress - FLASH_PEAK) / (1.0 - FLASH_PEAK);
        1.0 - fade * fade
    }
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

fn schedule_tick() -> iced::Task<Message> {
    iced::Task::perform(
        async {
            let (tx, rx) = futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                std::thread::sleep(TICK_INTERVAL);
                let _ = tx.send(());
            });
            let _ = rx.await;
        },
        |_| Message::BellTick,
    )
}
