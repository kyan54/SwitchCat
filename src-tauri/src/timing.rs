use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

static NEXT_SPAN: AtomicU64 = AtomicU64::new(1);

/// Log before execution as well as on completion so a stalled stage stays visible.
/// Only caller-supplied labels are logged; command arguments and output may contain secrets.
pub fn measure<T, E>(
    profile: &str,
    stage: &str,
    operation: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let span = NEXT_SPAN.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();
    log::info!("timing span={span} profile={profile} stage={stage} started");
    let result = operation();
    log::info!(
        "timing span={span} profile={profile} stage={stage} elapsed_ms={} ok={}",
        started.elapsed().as_millis(),
        result.is_ok()
    );
    result
}
