//! Transfer the paired producers after their final menu work has published.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Handoff {
    pub scene_revision: u64,
    pub foreground_revision: u64,
    pub extent: [u32; 2],
}
impl Handoff {
    pub fn ready(&self, scene_published: u64, foreground_published: u64) -> bool {
        scene_published >= self.scene_revision && foreground_published >= self.foreground_revision
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn foreground_clear_cannot_overtake_queued_menu_background() {
        let handoff = Handoff {
            scene_revision: 12,
            foreground_revision: 13,
            extent: [640, 360],
        };
        assert!(!handoff.ready(11, 13));
        assert!(!handoff.ready(12, 12));
        assert!(handoff.ready(12, 13));
        let resized = Handoff {
            foreground_revision: 14,
            extent: [800, 450],
            ..handoff
        };
        assert!(!resized.ready(12, 13));
        assert!(resized.ready(12, 14));
    }
}
