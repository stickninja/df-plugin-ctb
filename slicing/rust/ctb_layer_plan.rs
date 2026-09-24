//! Validated, resolved file-order settings shared with the frontend inspector.
use super::ctb_types::CtbTimingModel;
use crate::{engine::SlicerV3Error, types::SliceJobV3};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CtbResolvedLayer {
    pub position_z_mm: f32,
    pub exposure_sec: f32,
    pub light_off_delay_sec: f32,
    pub wait_time_before_cure_sec: f32,
    pub wait_time_after_cure_sec: f32,
    pub wait_time_after_lift_sec: f32,
    pub lift_distance_mm: f32,
    pub lift_distance2_mm: f32,
    pub lift_speed_mm_min: f32,
    pub lift_speed2_mm_min: f32,
    pub retract_distance2_mm: f32,
    pub retract_speed_mm_min: f32,
    pub retract_speed2_mm_min: f32,
    pub pwm: u16,
    pub is_dummy: bool,
    pub model_layer_number: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CtbLayerPlan {
    pub version: u32,
    pub model_layer_count: u32,
    pub startup_dummy: bool,
    pub bottom_defaults: Option<CtbTimingDefaults>,
    pub normal_defaults: Option<CtbTimingDefaults>,
    pub layers: Vec<CtbResolvedLayer>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CtbTimingDefaults {
    light_off_delay_sec: f32,
    wait_time_before_cure_sec: f32,
    wait_time_after_cure_sec: f32,
    wait_time_after_lift_sec: f32,
}

fn invalid(message: impl std::fmt::Display) -> SlicerV3Error {
    SlicerV3Error::UnsupportedOutput(format!("Invalid CTB layer plan: {message}"))
}

pub(super) fn parse(job: &SliceJobV3) -> Result<Option<CtbLayerPlan>, SlicerV3Error> {
    let Ok(meta) = serde_json::from_str::<serde_json::Value>(&job.metadata_json) else {
        return Ok(None);
    };
    let Some(value) = meta.get("ctb").and_then(|v| v.get("layerPlanV1")) else {
        return Ok(None);
    };
    let plan: CtbLayerPlan = serde_json::from_value(value.clone()).map_err(invalid)?;
    let build = super::ctb_metadata::parse_ctb_build_model_from_job(job);
    if !(4..=5).contains(&build.version) {
        return Err(invalid("requires CTB v4 or v5"));
    }
    if plan.version != 1
        || plan.model_layer_count != job.total_layers
        || plan.model_layer_count == 0
        || !job.layer_height_mm.is_finite()
        || job.layer_height_mm <= 0.0
    {
        return Err(invalid("version or model layer count does not match job"));
    }
    let offset = usize::from(plan.startup_dummy);
    for defaults in [&plan.bottom_defaults, &plan.normal_defaults]
        .into_iter()
        .flatten()
    {
        if [
            defaults.light_off_delay_sec,
            defaults.wait_time_before_cure_sec,
            defaults.wait_time_after_cure_sec,
            defaults.wait_time_after_lift_sec,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(invalid("invalid timing defaults"));
        }
    }
    if plan.layers.len() != plan.model_layer_count as usize + offset {
        return Err(invalid("file layer count does not match plan"));
    }
    for (index, layer) in plan.layers.iter().enumerate() {
        let dummy = offset == 1 && index == 0;
        let model_number = if dummy {
            None
        } else {
            Some((index + 1 - offset) as u32)
        };
        if layer.is_dummy != dummy || layer.model_layer_number != model_number {
            return Err(invalid(format!(
                "layer {} has inconsistent identity",
                index + 1
            )));
        }
        let values = [
            layer.position_z_mm,
            layer.exposure_sec,
            layer.light_off_delay_sec,
            layer.wait_time_before_cure_sec,
            layer.wait_time_after_cure_sec,
            layer.wait_time_after_lift_sec,
            layer.lift_distance_mm,
            layer.lift_distance2_mm,
            layer.lift_speed_mm_min,
            layer.lift_speed2_mm_min,
            layer.retract_distance2_mm,
            layer.retract_speed_mm_min,
            layer.retract_speed2_mm_min,
        ];
        if values.iter().any(|v| !v.is_finite() || *v < 0.0) || layer.pwm > 255 {
            return Err(invalid(format!(
                "layer {} contains invalid numeric settings",
                index + 1
            )));
        }
        let expected_z = model_number.unwrap_or(1) as f32 * job.layer_height_mm;
        if (layer.position_z_mm - expected_z).abs()
            > 0.0001_f32.max(expected_z.abs() * f32::EPSILON * 4.0)
        {
            return Err(invalid(format!(
                "layer {} changes physical model Z",
                index + 1
            )));
        }
        let total_lift = layer.lift_distance_mm + layer.lift_distance2_mm;
        if !total_lift.is_finite()
            || layer.retract_distance2_mm > total_lift + 0.00001
            || (layer.lift_distance_mm > 0.0 && layer.lift_speed_mm_min == 0.0)
            || (layer.lift_distance2_mm > 0.0 && layer.lift_speed2_mm_min == 0.0)
            || (total_lift > layer.retract_distance2_mm && layer.retract_speed_mm_min == 0.0)
            || (layer.retract_distance2_mm > 0.0 && layer.retract_speed2_mm_min == 0.0)
        {
            return Err(invalid(format!(
                "layer {} contains impossible motion settings",
                index + 1
            )));
        }
    }
    Ok(Some(plan))
}

impl CtbLayerPlan {
    pub fn apply_header(&self, timing: &mut CtbTimingModel) {
        if let Some(defaults) = &self.normal_defaults {
            timing.light_off_delay_sec = defaults.light_off_delay_sec;
            timing.wait_time_before_cure_sec = defaults.wait_time_before_cure_sec;
            timing.wait_time_after_cure_sec = defaults.wait_time_after_cure_sec;
            timing.wait_time_after_lift_sec = defaults.wait_time_after_lift_sec;
        }
        if let Some(defaults) = &self.bottom_defaults {
            timing.bottom_light_off_delay_sec = defaults.light_off_delay_sec;
            timing.bottom_wait_time_before_cure_sec = defaults.wait_time_before_cure_sec;
            timing.bottom_wait_time_after_cure_sec = defaults.wait_time_after_cure_sec;
            timing.bottom_wait_time_after_lift_sec = defaults.wait_time_after_lift_sec;
        }
        // The leading dummy belongs to the firmware bottom-layer prefix; the
        // actual real burn-in and transition records retain their resolved values.
        if timing.bottom_layer_count > 0 {
            timing.bottom_layer_count = timing
                .bottom_layer_count
                .saturating_add(u32::from(self.startup_dummy));
        }
        timing.wait_time_bottom_layer_count = timing
            .wait_time_bottom_layer_count
            .saturating_add(u32::from(self.startup_dummy));
    }

    pub fn print_time_seconds(&self) -> u32 {
        let seconds: f64 = self
            .layers
            .iter()
            .map(|layer| {
                let movement = |distance: f32, speed: f32| {
                    if distance > 0.0 && speed > 0.0 {
                        distance as f64 / speed as f64 * 60.0
                    } else {
                        0.0
                    }
                };
                let motion = movement(layer.lift_distance_mm, layer.lift_speed_mm_min)
                    + movement(layer.lift_distance2_mm, layer.lift_speed2_mm_min)
                    + movement(
                        (layer.lift_distance_mm + layer.lift_distance2_mm
                            - layer.retract_distance2_mm)
                            .max(0.0),
                        layer.retract_speed_mm_min,
                    )
                    + movement(layer.retract_distance2_mm, layer.retract_speed2_mm_min);
                layer.exposure_sec as f64
                    + (layer.light_off_delay_sec as f64).max(motion)
                    + layer.wait_time_before_cure_sec as f64
                    + layer.wait_time_after_cure_sec as f64
                    + layer.wait_time_after_lift_sec as f64
            })
            .sum();
        seconds.round().clamp(0.0, u32::MAX as f64) as u32
    }
}
