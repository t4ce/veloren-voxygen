//! Named cinematic stages; admission and networking deliberately live elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraStage {
    FadeToBlack,
    FaceBlackHold,
    FaceReveal,
    FaceApproach,
    PerspectiveBlackHold,
    LookUp,
    WhiteHold,
    ArrivalReveal,
    Complete,
}
#[derive(Clone, Copy, Debug)]
pub struct Cinematic {
    pub stage: CameraStage,
    elapsed: f32,
}
impl Default for Cinematic {
    fn default() -> Self {
        Self {
            stage: CameraStage::FadeToBlack,
            elapsed: 0.0,
        }
    }
}
impl Cinematic {
    pub fn duration(&self) -> f32 {
        use CameraStage::*;
        match self.stage {
            FadeToBlack | FaceReveal => 0.5,
            FaceBlackHold | PerspectiveBlackHold => 1.0,
            FaceApproach => 1.5,
            LookUp => 2.5,
            WhiteHold => 0.25,
            ArrivalReveal => 5.0,
            Complete => f32::INFINITY,
        }
    }
    pub fn progress(&self) -> f32 {
        (self.elapsed / self.duration()).clamp(0.0, 1.0)
    }
    pub fn ramp(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
    pub fn advance(&mut self, dt: f32) {
        if !dt.is_finite() || dt < 0.0 {
            return;
        }
        self.elapsed += dt;
        // Hold at WhiteHold until the frontend commits the prepared session.
        while self.elapsed >= self.duration() && self.stage != CameraStage::WhiteHold {
            self.elapsed -= self.duration();
            use CameraStage::*;
            self.stage = match self.stage {
                FadeToBlack => FaceBlackHold,
                FaceBlackHold => FaceReveal,
                FaceReveal => FaceApproach,
                FaceApproach => PerspectiveBlackHold,
                PerspectiveBlackHold => LookUp,
                LookUp => WhiteHold,
                ArrivalReveal => Complete,
                Complete | WhiteHold => break,
            };
        }
    }
    pub fn ready_to_swap(&self) -> bool {
        self.stage == CameraStage::WhiteHold && self.elapsed >= self.duration()
    }
    pub fn arrived(&mut self) {
        self.stage = CameraStage::ArrivalReveal;
        self.elapsed = 0.0;
    }
    /// Signed hardware LUT fade: -1 full black; +1 full white; 0 prior display state.
    pub fn display_fade(&self) -> f32 {
        use CameraStage::*;
        match self.stage {
            FadeToBlack => -Self::ramp(self.progress()),
            FaceBlackHold | PerspectiveBlackHold => -1.0,
            FaceReveal => Self::ramp(self.progress()) - 1.0,
            LookUp => {
                let p = self.progress();
                if p < 0.2 {
                    Self::ramp(p / 0.2) - 1.0
                } else {
                    Self::ramp((p - 0.6) / 0.4)
                }
            }
            WhiteHold => 1.0,
            ArrivalReveal => 1.0 - Self::ramp(self.elapsed / 0.5),
            FaceApproach | Complete => 0.0,
        }
    }
    pub fn arrival_overlay(&self) -> Option<(f32, f32)> {
        (self.stage == CameraStage::ArrivalReveal).then(|| {
            let p = Self::ramp(self.progress());
            (1.0 - p, (1.0 - p).max(0.01))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_never_advances_past_white_without_commit() {
        let mut c = Cinematic::default();
        c.advance(100.0);
        assert_eq!(c.stage, CameraStage::WhiteHold);
        assert!(c.ready_to_swap());
        assert_eq!(c.display_fade(), 1.0);
        c.arrived();
        c.advance(5.0);
        assert_eq!(c.stage, CameraStage::Complete);
        assert_eq!(c.display_fade(), 0.0);
    }
    #[test]
    fn black_holds_and_reveal_have_requested_durations() {
        let mut c = Cinematic::default();
        c.advance(0.5);
        assert_eq!(c.stage, CameraStage::FaceBlackHold);
        assert_eq!(c.display_fade(), -1.0);
        c.advance(1.0);
        assert_eq!(c.stage, CameraStage::FaceReveal);
        c.advance(0.5);
        assert_eq!(c.stage, CameraStage::FaceApproach);
        assert_eq!(c.display_fade(), 0.0);
    }
    #[test]
    fn white_starts_only_after_sixty_percent_of_look_up() {
        let mut c = Cinematic {
            stage: CameraStage::LookUp,
            elapsed: 1.5,
        };
        assert_eq!(c.display_fade(), 0.0);
        c.advance(0.5);
        assert!(c.display_fade() > 0.0 && c.display_fade() < 1.0);
    }
    #[test]
    fn arrival_shrinks_and_fades_for_five_seconds() {
        let mut c = Cinematic::default();
        c.arrived();
        assert_eq!(c.arrival_overlay(), Some((1.0, 1.0)));
        c.advance(2.5);
        assert_eq!(c.arrival_overlay(), Some((0.5, 0.5)));
        c.advance(2.5);
        assert_eq!(c.arrival_overlay(), None);
    }
}
