//! Frame identity remains attached through publication retries and scanout proof.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FrameInfo {
    pub sequence: u64,
    pub vertices: usize,
    pub extent: [u32; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Publication {
    pub frame: FrameInfo,
    pub serial: Option<u64>,
}
#[derive(Default, Debug)]
pub(super) struct Report {
    pub published: Option<Publication>,
    pub surflive: Option<Publication>,
    pub busy: bool,
}
#[derive(Default, Debug)]
pub(super) struct Receipt {
    pending: Option<Publication>,
}
impl Receipt {
    pub fn arm(&mut self, publication: Publication) {
        assert!(self.pending.is_none());
        assert!(publication.serial.is_some_and(|s| s != 0));
        self.pending = Some(publication);
    }
    pub fn pending(&self) -> Option<Publication> {
        self.pending
    }
    pub fn observe(&mut self, serial: u64, presented: bool) -> Option<Publication> {
        if presented && self.pending.is_some_and(|p| p.serial == Some(serial)) {
            self.pending.take()
        } else {
            None
        }
    }
    pub fn abandon(&mut self) {
        self.pending = None;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn producer_completion_and_other_receipts_do_not_prove_this_frame() {
        let frame = FrameInfo {
            sequence: 4,
            vertices: 16,
            extent: [640, 360],
        };
        let publication = Publication {
            frame,
            serial: Some(12),
        };
        let mut receipt = Receipt::default();
        receipt.arm(publication);
        assert!(receipt.observe(12, false).is_none());
        assert!(receipt.observe(11, true).is_none());
        assert_eq!(receipt.observe(12, true), Some(publication));
        assert!(receipt.pending().is_none());
    }
    #[test]
    fn retries_preserve_original_extent_and_geometry_identity() {
        let mut receipt = Receipt::default();
        let publication = Publication {
            frame: FrameInfo {
                sequence: 3,
                vertices: 8,
                extent: [640, 360],
            },
            serial: Some(9),
        };
        receipt.arm(publication);
        let newer = FrameInfo {
            sequence: 4,
            vertices: 0,
            extent: [800, 450],
        };
        assert_ne!(receipt.pending().unwrap().frame, newer);
        assert_eq!(receipt.observe(9, true).unwrap().frame.vertices, 8);
        receipt.arm(Publication {
            frame: newer,
            serial: Some(10),
        });
        receipt.abandon();
        assert!(receipt.observe(10, true).is_none());
    }
}
