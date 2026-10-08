//! Character-selector execution observations, independent of its frame loop.
//! Only atomics run on the observed thread. The existing Tokio runtime samples
//! them; a missing watchdog heartbeat is not proof that the frame loop stopped.
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[repr(u8)]
#[derive(Clone, Copy)]
pub(crate) enum Stage {
    BetweenFrames, Frame, WindowEvents, Ui, SkyMailbox, ClientTick, ClientCleanup,
    GeneralMessages, PingMessages, CharacterMessages, InGameMessages,
    TerrainMessages, NetworkEvents, WorldSimulation, FramePacing, GlobalMaintain,
}
const LABELS: [&str; 16] = [
    "between-frames", "frame", "window-events", "ui", "sky-mailbox", "client-tick",
    "client-cleanup", "general-messages", "ping-messages", "character-messages",
    "in-game-messages", "terrain-messages", "network-events", "world-simulation",
    "frame-pacing", "global-maintain",
];
static ACTIVE: AtomicBool = AtomicBool::new(false);
// Stage and transition counter share one atomic snapshot. A stage repeated in
// successive frames is progress; a parked frame cannot fake it by being polled.
static STAMP: AtomicU64 = AtomicU64::new(0);
static FRAMES: AtomicU64 = AtomicU64::new(0);

fn transition(stage: u8) -> u8 {
    let old = STAMP.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
        Some((old.wrapping_add(256) & !255) | u64::from(stage))
    }).unwrap();
    (old & 255) as u8
}

pub(crate) struct Scope(Option<u8>);
impl Drop for Scope {
    fn drop(&mut self) {
        if let Some(previous) = self.0 { transition(previous); }
    }
}

pub(crate) fn stage(stage: Stage) -> Scope {
    Scope(ACTIVE.load(Ordering::Relaxed).then(|| transition(stage as u8)))
}

pub(crate) fn frame(character_selection: bool) -> Scope {
    ACTIVE.store(character_selection, Ordering::Relaxed);
    if character_selection {
        FRAMES.fetch_add(1, Ordering::Relaxed);
    }
    stage(Stage::Frame)
}

pub fn start(runtime: &tokio::runtime::Runtime) {
    runtime.spawn(async {
        let mut last = 0;
        let mut unchanged = 0u32;
        let mut samples = 0u64;
        loop {
            tokio::time::sleep(core::time::Duration::from_secs(1)).await;
            if !ACTIVE.load(Ordering::Relaxed) {
                unchanged = 0;
                last = STAMP.load(Ordering::Relaxed);
                continue;
            }
            let stamp = STAMP.load(Ordering::Relaxed);
            unchanged = if last == stamp { unchanged.saturating_add(1) } else { 0 };
            last = stamp;
            samples += 1;
            let label = LABELS[(stamp & 255) as usize];
            if unchanged >= 2 && (unchanged == 2 || unchanged % 5 == 0) {
                tracing::warn!(target: "voxy_selection_progress",
                    stage = label, transitions = stamp >> 8,
                    frames = FRAMES.load(Ordering::Relaxed), unchanged_samples = unchanged,
                    "Character selector execution has not crossed an observed boundary");
            } else if samples % 10 == 0 {
                tracing::info!(target: "voxy_selection_progress",
                    stage = label, transitions = stamp >> 8,
                    frames = FRAMES.load(Ordering::Relaxed), unchanged_samples = unchanged,
                    "Character selector watchdog heartbeat");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_scopes_restore_stage_and_count_actual_progress() {
        let frame_guard = frame(true);
        let before = STAMP.load(Ordering::Relaxed);
        {
            let _tick = stage(Stage::ClientTick);
            let _messages = stage(Stage::GeneralMessages);
            assert_eq!(STAMP.load(Ordering::Relaxed) & 255, Stage::GeneralMessages as u64);
        }
        let after = STAMP.load(Ordering::Relaxed);
        assert_eq!(after & 255, Stage::Frame as u64);
        assert!(after >> 8 > before >> 8);
        drop(frame_guard);
        assert_eq!(STAMP.load(Ordering::Relaxed) & 255, Stage::BetweenFrames as u64);
        drop(frame(false));
        let idle = STAMP.load(Ordering::Relaxed);
        drop(stage(Stage::WorldSimulation));
        assert_eq!(STAMP.load(Ordering::Relaxed), idle);
    }
}
