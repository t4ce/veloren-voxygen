use super::{super::Animation, QuadrupedLowSkeleton, SkeletonAttr, quadruped_low_beta};
use common::states::utils::StageSection;
//use core::ops::Rem;

pub struct BetaAnimation;

impl Animation for BetaAnimation {
    type Dependency<'a> = (f32, f32, StageSection, f32);
    type Skeleton = QuadrupedLowSkeleton;


    
    fn update_skeleton_inner(
        skeleton: &Self::Skeleton,
        (_velocity, global_time, stage_section, timer): Self::Dependency<'_>,
        anim_time: f32,
        _rate: &mut f32,
        s_a: &SkeletonAttr,
    ) -> Self::Skeleton {
        let mut next = (*skeleton).clone();

        quadruped_low_beta(&mut next, s_a, stage_section, anim_time, global_time, timer);

        next
    }
}
