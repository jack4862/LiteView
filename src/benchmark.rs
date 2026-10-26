use std::{
    path::Path,
    time::{Duration, Instant},
};

use crate::loader::{self, IccStatus, LoadError, LoadOptions, SourceFormat};

#[derive(Debug, Clone)]
pub struct BenchmarkReport {
    pub format: SourceFormat,
    pub source_dimensions: [u32; 2],
    pub display_dimensions: [u32; 2],
    pub preview_dimensions: [u32; 2],
    pub file_size: u64,
    pub icc_status: IccStatus,
    pub downsampled: bool,
    pub iterations: usize,
    pub min: Duration,
    pub p50: Duration,
    pub p95: Duration,
    pub max: Duration,
}

pub fn run(path: &Path, iterations: usize) -> Result<BenchmarkReport, LoadError> {
    let iterations = iterations.max(1);
    let mut samples = Vec::with_capacity(iterations);
    let mut first = None;

    for _ in 0..iterations {
        let started = Instant::now();
        let loaded = loader::load(path, &LoadOptions::default())?;
        samples.push(started.elapsed());
        if first.is_none() {
            first = Some((
                loaded.format,
                loaded.source_dimensions,
                loaded.display_dimensions,
                loaded.preview_dimensions,
                loaded.file_size,
                loaded.icc_status,
                loaded.downsampled,
            ));
        }
    }

    samples.sort_unstable();
    let (
        format,
        source_dimensions,
        display_dimensions,
        preview_dimensions,
        file_size,
        icc_status,
        downsampled,
    ) = first.expect("at least one benchmark sample");

    Ok(BenchmarkReport {
        format,
        source_dimensions,
        display_dimensions,
        preview_dimensions,
        file_size,
        icc_status,
        downsampled,
        iterations,
        min: samples[0],
        p50: percentile(&samples, 50),
        p95: percentile(&samples, 95),
        max: *samples.last().expect("at least one benchmark sample"),
    })
}

fn percentile(sorted: &[Duration], percentile: usize) -> Duration {
    let rank = (sorted.len() * percentile).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::percentile;
    use std::time::Duration;

    #[test]
    fn calculates_nearest_rank_percentiles() {
        let samples: Vec<_> = (1..=100).map(Duration::from_millis).collect();

        assert_eq!(percentile(&samples, 50), Duration::from_millis(50));
        assert_eq!(percentile(&samples, 95), Duration::from_millis(95));
        assert_eq!(percentile(&samples[..1], 95), Duration::from_millis(1));
    }
}
