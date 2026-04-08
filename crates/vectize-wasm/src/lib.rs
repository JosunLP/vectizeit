use js_sys::{Function, Uint8Array};
use serde::{Deserialize, Serialize};
use vectize::{QualityPreset, TraceProgressUpdate, TraceStageMetrics, Tracer, TracingConfig};
use wasm_bindgen::prelude::*;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct WasmTracingConfig {
    preset: Option<String>,
    color_count: Option<u16>,
    simplification_tolerance: Option<f64>,
    min_region_area: Option<f64>,
    smoothing_strength: Option<f64>,
    corner_sensitivity: Option<f64>,
    alpha_threshold: Option<u8>,
    despeckle_threshold: Option<f64>,
    enable_denoising: Option<bool>,
    enable_preprocessing: Option<bool>,
    background_color: Option<String>,
    enable_svg_gradients: Option<bool>,
    tile_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WasmTracingResult {
    svg: String,
    width: u32,
    height: u32,
    palette: Vec<String>,
    regions: Vec<WasmRegionSummary>,
    stage_metrics: Option<WasmTraceStageMetrics>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WasmRegionSummary {
    color: String,
    contour_count: usize,
    hole_count: usize,
    total_points: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WasmTraceStageMetrics {
    contours_extracted: usize,
    holes_extracted: usize,
    points_extracted: usize,
    invalid_contours_discarded: usize,
    contours_after_despeckle: usize,
    holes_after_despeckle: usize,
    points_after_despeckle: usize,
    contours_simplified_away: usize,
    contours_filtered_min_area: usize,
    contours_suppressed_background: usize,
    contours_emitted: usize,
    holes_emitted: usize,
    points_emitted: usize,
    regions_emitted: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WasmTraceProgressUpdate {
    stage: String,
    completed_stages: usize,
    total_stages: usize,
}

#[wasm_bindgen(js_name = traceBytesSvg)]
pub fn trace_bytes_svg(bytes: Uint8Array) -> std::result::Result<String, JsValue> {
    let bytes = bytes.to_vec();
    vectize::trace_bytes(&bytes).map_err(to_js_error)
}

#[wasm_bindgen(js_name = traceBytes)]
pub fn trace_bytes_default(bytes: Uint8Array) -> std::result::Result<JsValue, JsValue> {
    let bytes = bytes.to_vec();
    let result = Tracer::with_preset(QualityPreset::Balanced)
        .trace_bytes_result(&bytes)
        .map_err(to_js_error)?;
    serialize_result(&result)
}

#[wasm_bindgen(js_name = traceBytesWithPreset)]
pub fn trace_bytes_with_preset(
    bytes: Uint8Array,
    preset: String,
) -> std::result::Result<JsValue, JsValue> {
    let bytes = bytes.to_vec();
    let preset = QualityPreset::parse(&preset)
        .ok_or_else(|| JsValue::from_str("invalid preset; expected fast, balanced, or high"))?;
    let result = Tracer::with_preset(preset)
        .trace_bytes_result(&bytes)
        .map_err(to_js_error)?;
    serialize_result(&result)
}

#[wasm_bindgen(js_name = traceBytesWithConfig)]
pub fn trace_bytes_with_config(
    bytes: Uint8Array,
    config: JsValue,
) -> std::result::Result<JsValue, JsValue> {
    let bytes = bytes.to_vec();
    let tracer = Tracer::new(parse_config(config)?);
    let result = tracer.trace_bytes_result(&bytes).map_err(to_js_error)?;
    serialize_result(&result)
}

#[wasm_bindgen(js_name = traceBytesWithProgress)]
pub fn trace_bytes_with_progress(
    bytes: Uint8Array,
    config: JsValue,
    callback: Function,
) -> std::result::Result<JsValue, JsValue> {
    let bytes = bytes.to_vec();
    let tracer = Tracer::new(parse_config(config)?);
    let result = tracer
        .trace_bytes_result_with_progress(&bytes, |update| {
            if let Ok(value) = serde_wasm_bindgen::to_value(&WasmTraceProgressUpdate::from(update))
            {
                let _ = callback.call1(&JsValue::NULL, &value);
            }
        })
        .map_err(to_js_error)?;

    serialize_result(&result)
}

impl WasmTracingConfig {
    fn into_config(self) -> std::result::Result<TracingConfig, String> {
        let mut config = if let Some(preset) = self.preset {
            QualityPreset::parse(&preset)
                .ok_or_else(|| "invalid preset; expected fast, balanced, or high".to_string())?
                .to_config()
        } else {
            TracingConfig::default()
        };

        if let Some(value) = self.color_count {
            config.color_count = value;
        }
        if let Some(value) = self.simplification_tolerance {
            config.simplification_tolerance = value;
        }
        if let Some(value) = self.min_region_area {
            config.min_region_area = value;
        }
        if let Some(value) = self.smoothing_strength {
            config.smoothing_strength = value;
        }
        if let Some(value) = self.corner_sensitivity {
            config.corner_sensitivity = value;
        }
        if let Some(value) = self.alpha_threshold {
            config.alpha_threshold = value;
        }
        if let Some(value) = self.despeckle_threshold {
            config.despeckle_threshold = value;
        }
        if let Some(value) = self.enable_denoising {
            config.enable_denoising = value;
        }
        if let Some(value) = self.enable_preprocessing {
            config.enable_preprocessing = value;
        }
        if let Some(value) = self.background_color {
            config.background_color = Some(TracingConfig::parse_hex_color(&value)?);
        }
        if let Some(value) = self.enable_svg_gradients {
            config.enable_svg_gradients = value;
        }
        if let Some(value) = self.tile_size {
            config.tile_size = Some(value);
        }

        config.validate().map_err(|error| error.to_string())?;
        Ok(config)
    }
}

impl From<&vectize::TracingResult> for WasmTracingResult {
    fn from(result: &vectize::TracingResult) -> Self {
        Self {
            svg: result.svg().to_string(),
            width: result.width(),
            height: result.height(),
            palette: result
                .debug()
                .palette()
                .iter()
                .map(|color| color.to_hex())
                .collect(),
            regions: result
                .debug()
                .regions()
                .iter()
                .map(|region| WasmRegionSummary {
                    color: region.color().to_hex(),
                    contour_count: region.contour_count(),
                    hole_count: region.hole_count(),
                    total_points: region.total_points(),
                })
                .collect(),
            stage_metrics: result.stage_metrics().map(WasmTraceStageMetrics::from),
        }
    }
}

impl From<&TraceStageMetrics> for WasmTraceStageMetrics {
    fn from(metrics: &TraceStageMetrics) -> Self {
        Self {
            contours_extracted: metrics.contours_extracted(),
            holes_extracted: metrics.holes_extracted(),
            points_extracted: metrics.points_extracted(),
            invalid_contours_discarded: metrics.invalid_contours_discarded(),
            contours_after_despeckle: metrics.contours_after_despeckle(),
            holes_after_despeckle: metrics.holes_after_despeckle(),
            points_after_despeckle: metrics.points_after_despeckle(),
            contours_simplified_away: metrics.contours_simplified_away(),
            contours_filtered_min_area: metrics.contours_filtered_min_area(),
            contours_suppressed_background: metrics.contours_suppressed_background(),
            contours_emitted: metrics.contours_emitted(),
            holes_emitted: metrics.holes_emitted(),
            points_emitted: metrics.points_emitted(),
            regions_emitted: metrics.regions_emitted(),
        }
    }
}

impl From<TraceProgressUpdate> for WasmTraceProgressUpdate {
    fn from(update: TraceProgressUpdate) -> Self {
        Self {
            stage: format!("{:?}", update.stage()),
            completed_stages: update.completed_stages(),
            total_stages: update.total_stages(),
        }
    }
}

fn parse_config(config: JsValue) -> std::result::Result<TracingConfig, JsValue> {
    if config.is_null() || config.is_undefined() {
        return Ok(TracingConfig::default());
    }

    let config: WasmTracingConfig = serde_wasm_bindgen::from_value(config)
        .map_err(|error| JsValue::from_str(&format!("invalid config: {error}")))?;
    config
        .into_config()
        .map_err(|error| JsValue::from_str(&error))
}

fn serialize_result(result: &vectize::TracingResult) -> std::result::Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(&WasmTracingResult::from(result))
        .map_err(|error| JsValue::from_str(&format!("failed to serialize result: {error}")))
}

fn to_js_error(error: vectize::VectizeError) -> JsValue {
    JsValue::from_str(&error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_config_conversion_accepts_new_runtime_knobs() {
        let config = WasmTracingConfig {
            preset: Some("high".to_string()),
            background_color: Some("#123456".to_string()),
            enable_svg_gradients: Some(true),
            tile_size: Some(256),
            ..WasmTracingConfig::default()
        }
        .into_config()
        .unwrap();

        assert_eq!(config.background_color, Some((0x12, 0x34, 0x56)));
        assert!(config.enable_svg_gradients);
        assert_eq!(config.tile_size, Some(256));
        assert!(matches!(config.quality_preset, QualityPreset::High));
    }
}
