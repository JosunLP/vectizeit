//! Progress reporting types for tracing runs.

use crate::config::{QualityPreset, TracingConfig};

/// One completed stage in the tracing pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TraceStage {
    /// The source image has been decoded successfully.
    Loaded,
    /// Preprocessing (normalization / denoising) finished.
    Preprocessed,
    /// Alpha compositing against the configured background finished.
    Composited,
    /// Optional higher-resolution tracing resample finished.
    Resampled,
    /// Color quantization / segmentation finished.
    Quantized,
    /// Optional SVG gradient approximation finished.
    GradientsApproximated,
    /// Contour extraction finished.
    ContoursExtracted,
    /// Despeckling finished.
    Despeckled,
    /// SVG generation finished.
    SvgGenerated,
    /// The full tracing run is complete.
    Completed,
}

/// One progress callback update emitted after a stage completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceProgressUpdate {
    stage: TraceStage,
    completed_stages: usize,
    total_stages: usize,
}

impl TraceProgressUpdate {
    /// Create a new progress update.
    pub fn new(stage: TraceStage, completed_stages: usize, total_stages: usize) -> Self {
        Self {
            stage,
            completed_stages,
            total_stages,
        }
    }

    /// Return the stage that just completed.
    pub fn stage(&self) -> TraceStage {
        self.stage
    }

    /// Return how many stages have completed so far.
    pub fn completed_stages(&self) -> usize {
        self.completed_stages
    }

    /// Return the total number of stages expected for this run.
    pub fn total_stages(&self) -> usize {
        self.total_stages
    }
}

pub(crate) struct ProgressTracker<'a> {
    callback: Option<&'a mut dyn FnMut(TraceProgressUpdate)>,
    completed_stages: usize,
    total_stages: usize,
}

impl<'a> ProgressTracker<'a> {
    pub(crate) fn new(
        config: &TracingConfig,
        callback: Option<&'a mut dyn FnMut(TraceProgressUpdate)>,
    ) -> Self {
        Self {
            callback,
            completed_stages: 0,
            total_stages: total_stage_count(config),
        }
    }

    pub(crate) fn advance(&mut self, stage: TraceStage) {
        self.completed_stages += 1;

        if let Some(callback) = self.callback.as_deref_mut() {
            callback(TraceProgressUpdate::new(
                stage,
                self.completed_stages,
                self.total_stages,
            ));
        }
    }
}

fn total_stage_count(config: &TracingConfig) -> usize {
    let mut total = 8;

    if matches!(config.quality_preset, QualityPreset::High) {
        total += 1;
    }
    if config.enable_svg_gradients {
        total += 1;
    }

    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_stage_count_includes_optional_stages() {
        let balanced = TracingConfig::default();
        let mut high = QualityPreset::High.to_config();
        high.enable_svg_gradients = true;

        assert_eq!(total_stage_count(&balanced), 8);
        assert_eq!(total_stage_count(&high), 10);
    }
}
