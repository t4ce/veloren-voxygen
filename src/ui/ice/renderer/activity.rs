//! Interval counters. Durations measure caller wall time, including kernel waits;
//! they are not hardware GPU timestamps. Snapshots never gate rendering.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub(crate) fn micros(duration: Duration) -> u64 {
    duration.as_micros().min(u64::MAX as u128) as u64
}
#[derive(Default, Debug)]
pub(crate) struct UiActivity {
    pub updates: u64,
    pub input_events: u64,
    pub messages: u64,
    pub unchanged: u64,
    pub uncached_plans: u64,
    pub input_plans: u64,
    pub no_input_plans: u64,
    pub resize_invalidations: u64,
    pub layout_us: u64,
    pub compare_us: u64,
    pub prepare_us: u64,
}
#[derive(Default, Debug)]
pub(crate) struct PreparationActivity {
    pub images: u64,
    pub glyphs: u64,
    pub gradients: u64,
    pub bytes: u64,
}
macro_rules! producer_counters {
    ($($field:ident),* $(,)?) => {
        #[derive(Default)]
        pub(crate) struct ProducerCounters { $(pub $field: AtomicU64,)* }
        #[derive(Default, Debug)]
        pub(crate) struct ProducerActivity { $(pub $field: u64,)* }
        impl ProducerCounters {
            pub fn take(&self) -> ProducerActivity {
                ProducerActivity { $($field: self.$field.swap(0, Ordering::Relaxed),)* }
            }
        }
    };
}
producer_counters!(
    iterations,
    queued_replacements,
    unchanged,
    uploads,
    upload_bytes,
    upload_call_us,
    begins,
    draws,
    bcs_commands,
    compositor_commands,
    draw_call_us,
    publications,
    publish_call_us,
    busy_upload,
    busy_begin,
    busy_draw,
    busy_publish,
);
