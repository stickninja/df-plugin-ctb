use super::*;
use serde_json::{json, Value};

fn fixture(version: &str, dummy: bool) -> (SliceJobV3, Vec<CtbPreparedLayer>, Vec<Vec<u8>>) {
    let mut layers: Vec<Value> = (1..=4).map(|n| json!({
        "positionZMm": n as f32 * 0.05, "modelLayerNumber": n, "isDummy":false,
        "exposureSec": if n <= 2 { 20.0 } else if n == 3 { 11.0 } else { 2.0 },
        "lightOffDelaySec": 10.0+n as f32, "waitTimeBeforeCureSec":1.0+n as f32,
        "waitTimeAfterCureSec":0.3, "waitTimeAfterLiftSec":0.7,
        "liftDistanceMm":3.0,"liftDistance2Mm":2.0,"liftSpeedMmMin":60.0,"liftSpeed2MmMin":120.0,
        "retractDistance2Mm":1.0,"retractSpeedMmMin":180.0,"retractSpeed2MmMin":90.0,"pwm":230
    })).collect();
    if dummy {
        let mut first = layers[0].clone();
        for (key, value) in [
            ("positionZMm", 0.05),
            ("exposureSec", 0.01),
            ("lightOffDelaySec", 0.0),
            ("waitTimeBeforeCureSec", 0.0),
            ("waitTimeAfterCureSec", 0.0),
            ("waitTimeAfterLiftSec", 0.0),
            ("liftDistanceMm", 0.1),
            ("liftDistance2Mm", 0.0),
            ("retractDistance2Mm", 0.1),
        ] {
            first[key] = json!(value);
        }
        first["pwm"] = json!(1);
        first["isDummy"] = json!(true);
        first["modelLayerNumber"] = Value::Null;
        layers.insert(0, first);
    }
    let job = SliceJobV3 {
        output_format:".ctb".into(),format_version:Some(version.into()),source_width_px:4,source_height_px:4,
        width_px:4,height_px:4,build_width_mm:10.0,build_depth_mm:20.0,layer_height_mm:0.05,total_layers:4,
        metadata_json:json!({"ctb":{"settingsMode":"twostage","bottomLayerCount":2,"transitionLayerCount":1,
            "normalExposureSec":2.0,"bottomExposureSec":20.0,"layerPlanV1":{
            "version":1,"modelLayerCount":4,"startupDummy":dummy,
            "normalDefaults":{"lightOffDelaySec":19.0,"waitTimeBeforeCureSec":4.0,"waitTimeAfterCureSec":0.6,"waitTimeAfterLiftSec":0.8},
            "bottomDefaults":{"lightOffDelaySec":29.0,"waitTimeBeforeCureSec":6.0,"waitTimeAfterCureSec":0.9,"waitTimeAfterLiftSec":1.2},
            "layers":layers}}}).to_string(),
        ..Default::default()
    };
    let masks: Vec<Vec<u8>> = (0..4)
        .map(|index| {
            (0..16)
                .map(|p| if p == index + 5 { 255 } else { 0 })
                .collect()
        })
        .collect();
    let prepared = prepare_layers_for_ctb(
        &masks,
        false,
        127,
        parse_ctb_build_model_from_job(&job).layer_xor_key,
    );
    (job, prepared, masks)
}

#[test]
fn plan_records_roundtrip_all_four_ctb_variants_and_preserve_images() {
    for version in ["v4", "v5", "v4enc", "v5enc"] {
        for dummy in [false, true] {
            let (job, prepared, masks) = fixture(version, dummy);
            let bytes = build_ctb_container_bytes(&job, &prepared).unwrap();
            let offset = u32::from(dummy);
            let path = std::env::temp_dir().join(format!(
                "dragonfruit-ctb-test-{}-{version}-{dummy}.ctb",
                std::process::id()
            ));
            std::fs::write(&path, &bytes).unwrap();
            for n in 1..=4 {
                let actual = read_ctb_layer_settings_from_bytes(&bytes, n + offset).unwrap();
                assert_eq!(actual.layer_count, 4 + offset);
                assert_eq!(actual.bottom_layer_count, 2 + offset);
                assert!(actual.per_layer_settings);
                assert!((actual.position_z_mm - n as f32 * 0.05).abs() < 0.00001);
                assert_eq!(
                    actual.exposure_sec,
                    if n <= 2 {
                        20.0
                    } else if n == 3 {
                        11.0
                    } else {
                        2.0
                    }
                );
                assert_eq!(actual.light_off_delay_sec, 10.0 + n as f32);
                assert_eq!(actual.wait_time_before_cure_sec, 1.0 + n as f32);
                assert_eq!(actual.wait_time_after_cure_sec, 0.3);
                assert_eq!(actual.wait_time_after_lift_sec, 0.7);
                assert_eq!(actual.lift_distance_mm, 3.0);
                assert_eq!(actual.lift_distance2_mm, 2.0);
                assert_eq!(actual.retract_distance2_mm, 1.0);
                assert_eq!(actual.pwm, 230);
                let png = read_layer_preview_png(&path, n + offset).unwrap();
                let mut decoder = png::Decoder::new(std::io::Cursor::new(png))
                    .read_info()
                    .unwrap();
                let mut pixels = vec![0; decoder.output_buffer_size()];
                decoder.next_frame(&mut pixels).unwrap();
                assert_eq!(
                    pixels,
                    masks[n as usize - 1]
                        .iter()
                        .map(|v| v & 0xfe)
                        .collect::<Vec<_>>(),
                    "{version}: real pixels changed"
                );
            }
            if dummy {
                let actual = read_ctb_layer_settings_from_bytes(&bytes, 1).unwrap();
                assert_eq!(actual.position_z_mm, 0.05);
                assert_eq!(actual.pwm, 1);
                assert_eq!(actual.exposure_sec, 0.01);
                assert_eq!(actual.light_off_delay_sec, 0.0);
                assert_eq!(actual.lift_distance_mm, 0.1);
                let png = read_layer_preview_png(&path, 1).unwrap();
                let mut decoder = png::Decoder::new(std::io::Cursor::new(png))
                    .read_info()
                    .unwrap();
                let mut pixels = vec![0; decoder.output_buffer_size()];
                decoder.next_frame(&mut pixels).unwrap();
                assert_eq!(pixels.iter().filter(|v| **v != 0).count(), 1);
                assert_eq!(pixels[5], 128);
            }
            // Check physical print height in actual global header/settings.
            let height = if version.ends_with("enc") {
                let mut settings = bytes[48..336].to_vec();
                let (key, iv) = ctb_crypto::ctb_default_key_iv();
                ctb_crypto::ctb_decrypt_in_place_no_padding(&mut settings, &key, &iv).unwrap();
                assert_eq!(
                    f32::from_le_bytes(settings[48..52].try_into().unwrap()),
                    19.0
                );
                assert_eq!(
                    f32::from_le_bytes(settings[116..120].try_into().unwrap()),
                    29.0
                );
                assert_eq!(
                    f32::from_le_bytes(settings[216..220].try_into().unwrap()),
                    4.0
                );
                assert_eq!(
                    f32::from_le_bytes(settings[224..228].try_into().unwrap()),
                    0.6
                );
                f32::from_le_bytes(settings[32..36].try_into().unwrap())
            } else {
                assert_eq!(f32::from_le_bytes(bytes[44..48].try_into().unwrap()), 19.0);
                let params = u32::from_le_bytes(bytes[84..88].try_into().unwrap()) as usize;
                assert_eq!(
                    f32::from_le_bytes(bytes[params + 32..params + 36].try_into().unwrap()),
                    29.0
                );
                f32::from_le_bytes(bytes[28..32].try_into().unwrap())
            };
            assert_eq!(height, 0.2);
            if let Some(dir) = std::env::var_os("DF_CTB_FIXTURE_DIR") {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(
                    std::path::PathBuf::from(dir)
                        .join(format!("phase3-{version}-dummy-{dummy}.ctb")),
                    &bytes,
                )
                .unwrap();
            }
            std::fs::remove_file(path).unwrap();
        }
    }
}

#[test]
fn plan_accepts_javascript_z_rounding_at_full_printer_height() {
    for height in [0.03_f64, 0.05] {
        let (mut job, _, _) = fixture("v5", false);
        let mut meta: Value = serde_json::from_str(&job.metadata_json).unwrap();
        let template = meta["ctb"]["layerPlanV1"]["layers"][0].clone();
        let count = (250.0 / height).floor() as u32;
        meta["ctb"]["layerPlanV1"]["layers"] = json!((1..=count)
            .map(|n| {
                let mut layer = template.clone();
                layer["positionZMm"] = json!(n as f64 * height);
                layer["modelLayerNumber"] = json!(n);
                layer
            })
            .collect::<Vec<_>>());
        meta["ctb"]["layerPlanV1"]["modelLayerCount"] = json!(count);
        job.total_layers = count;
        job.layer_height_mm = height as f32;
        job.metadata_json = meta.to_string();
        assert!(ctb_layer_plan::parse(&job).is_ok());
    }
}

#[test]
fn dummy_bottom_prefix_preserves_zero_and_large_burn_in_counts() {
    for bottom_count in [0, 1, 7, 8, 20] {
        let (mut job, prepared, _) = fixture("v5", true);
        let mut meta: Value = serde_json::from_str(&job.metadata_json).unwrap();
        meta["ctb"]["bottomLayerCount"] = json!(bottom_count);
        job.metadata_json = meta.to_string();
        let bytes = build_ctb_container_bytes(&job, &prepared).unwrap();
        let actual = read_ctb_layer_settings_from_bytes(&bytes, 2).unwrap();
        assert_eq!(
            actual.bottom_layer_count,
            if bottom_count == 0 {
                0
            } else {
                bottom_count + 1
            }
        );
        assert_eq!(actual.exposure_sec, 20.0);
    }
}

#[test]
fn plan_time_estimate_keeps_explicit_waits_outside_lod_motion_maximum() {
    let (job, _, _) = fixture("v5", false);
    let plan = ctb_layer_plan::parse(&job).unwrap().unwrap();
    // Real records: exposures 53 + LOD 50 (all exceed 5.333s motion) + waits 18.
    assert_eq!(plan.print_time_seconds(), 121);
}

#[test]
fn plan_validation_rejects_wrong_version_identity_z_motion_and_count() {
    let (job, prepared, _) = fixture("v5", false);
    for mutate in [0, 1, 2, 3, 4] {
        let mut bad = job.clone();
        let mut meta: Value = serde_json::from_str(&bad.metadata_json).unwrap();
        match mutate {
            0 => meta["ctb"]["layerPlanV1"]["version"] = json!(2),
            1 => meta["ctb"]["layerPlanV1"]["layers"][0]["modelLayerNumber"] = json!(2),
            2 => meta["ctb"]["layerPlanV1"]["layers"][0]["positionZMm"] = json!(1.0),
            3 => meta["ctb"]["layerPlanV1"]["layers"][0]["liftSpeedMmMin"] = json!(0),
            _ => meta["ctb"]["layerPlanV1"]["modelLayerCount"] = json!(3),
        };
        bad.metadata_json = meta.to_string();
        assert!(build_ctb_container_bytes(&bad, &prepared).is_err());
    }
    for version in ["v2", "v3", "v3enc"] {
        let (job, prepared, _) = fixture(version, false);
        assert!(build_ctb_container_bytes(&job, &prepared).is_err());
    }
    assert!(read_ctb_layer_settings_from_bytes(&[0; 112], 1).is_err());
    let bytes = build_ctb_container_bytes(&job, &prepared).unwrap();
    assert!(read_ctb_layer_settings_from_bytes(&bytes, 0).is_err());
    assert!(read_ctb_layer_settings_from_bytes(&bytes, 5).is_err());
    assert!(read_ctb_layer_settings_from_bytes(&bytes[..100], 1).is_err());
}

#[test]
fn simple_plan_preserves_waits_and_zero_stage_two_independently() {
    let (mut job, prepared, _) = fixture("v4", false);
    let mut meta: Value = serde_json::from_str(&job.metadata_json).unwrap();
    meta["ctb"]["settingsMode"] = json!("simple");
    for layer in meta["ctb"]["layerPlanV1"]["layers"].as_array_mut().unwrap() {
        for field in [
            "liftDistance2Mm",
            "liftSpeed2MmMin",
            "retractDistance2Mm",
            "retractSpeed2MmMin",
        ] {
            layer[field] = json!(0.0);
        }
    }
    job.metadata_json = meta.to_string();
    let bytes = build_ctb_container_bytes(&job, &prepared).unwrap();
    let actual = read_ctb_layer_settings_from_bytes(&bytes, 1).unwrap();
    assert!(actual.per_layer_settings);
    assert_eq!(actual.lift_distance2_mm, 0.0);
    assert_eq!(actual.light_off_delay_sec, 11.0);
    assert_eq!(actual.wait_time_before_cure_sec, 2.0);
    assert_eq!(actual.wait_time_after_cure_sec, 0.3);
}

#[test]
fn pwm_records_preserve_zero_quantization_and_full_power_in_every_variant() {
    for version in ["v4", "v5", "v4enc", "v5enc"] {
        for dummy in [false, true] {
            let (mut job, prepared, _) = fixture(version, dummy);
            let mut meta: Value = serde_json::from_str(&job.metadata_json).unwrap();
            let offset = usize::from(dummy);
            let expected = [0, 1, 128, 255];
            for (index, pwm) in expected.iter().enumerate() {
                meta["ctb"]["layerPlanV1"]["layers"][index + offset]["pwm"] = json!(pwm);
            }
            job.metadata_json = meta.to_string();
            let bytes = build_ctb_container_bytes(&job, &prepared).unwrap();
            for (index, pwm) in expected.iter().enumerate() {
                let actual = read_ctb_layer_settings_from_bytes(&bytes, (index + offset + 1) as u32).unwrap();
                assert!(actual.per_layer_settings);
                assert_eq!(actual.pwm, *pwm, "{version}, model layer {}", index + 1);
            }
            if dummy {
                assert_eq!(read_ctb_layer_settings_from_bytes(&bytes, 1).unwrap().pwm, 1);
            }
            for invalid in [json!(-1), json!(256), json!(1.5), Value::Null] {
                meta["ctb"]["layerPlanV1"]["layers"][offset]["pwm"] = invalid;
                job.metadata_json = meta.to_string();
                assert!(build_ctb_container_bytes(&job, &prepared).is_err());
            }
        }
    }
}

#[test]
fn legacy_pwm_defaults_encode_independently_and_accept_historical_bottom_alias() {
    for version in ["v4", "v5", "v4enc", "v5enc"] {
        for mode in ["simple", "twostage", "allfields"] {
            for key in ["bottomProjectorPwmPercent", "bottomLayerProjectorPwmPercent"] {
                let (mut job, prepared, _) = fixture(version, false);
                let mut meta: Value = serde_json::from_str(&job.metadata_json).unwrap();
                meta["ctb"].as_object_mut().unwrap().remove("layerPlanV1");
                meta["ctb"]["settingsMode"] = json!(mode);
                meta["ctb"]["projectorPwmPercent"] = json!(100);
                meta["ctb"][key] = json!(80);
                job.metadata_json = meta.to_string();
                let bytes = build_ctb_container_bytes(&job, &prepared).unwrap();
                for (layer, expected) in [(1, 204), (2, 204), (3, 255), (4, 255)] {
                    assert_eq!(read_ctb_layer_settings_from_bytes(&bytes, layer).unwrap().pwm, expected,
                        "{version}, {mode}, {key}, layer {layer}");
                }
                // Canonical zero continues to mean full power, even if an alias is present.
                meta["ctb"]["bottomProjectorPwmPercent"] = json!(0);
                meta["ctb"]["bottomLayerProjectorPwmPercent"] = json!(80);
                job.metadata_json = meta.to_string();
                let bytes = build_ctb_container_bytes(&job, &prepared).unwrap();
                assert_eq!(read_ctb_layer_settings_from_bytes(&bytes, 1).unwrap().pwm, 255);
            }
        }
    }
}

/// Run after scripts/generate-phase4-ctb-plans.ts with DF_CTB_PHASE4_PLAN_DIR set.
/// Uses actual frontend resolution rather than reimplementing its range rules in Rust.
#[test]
#[ignore = "requires generated frontend plans in DF_CTB_PHASE4_PLAN_DIR"]
fn phase4_frontend_motion_plans_encode_and_decode() {
    let dir = std::path::PathBuf::from(
        std::env::var_os("DF_CTB_PHASE4_PLAN_DIR").expect("frontend plan directory"),
    );
    for version in ["v4", "v5", "v4enc", "v5enc"] {
        for mode in ["simple", "twostage"] {
            for dummy in [false, true] {
                let name = format!("phase4-{version}-{mode}-dummy-{dummy}");
                let generated: Value = serde_json::from_slice(
                    &std::fs::read(dir.join(format!("{name}.json"))).unwrap(),
                )
                .unwrap();
                let (mut job, prepared, _) = fixture(version, dummy);
                job.metadata_json = generated["metadata"].to_string();
                let plan = ctb_layer_plan::parse(&job).unwrap().unwrap();
                assert_eq!(
                    plan.print_time_seconds(),
                    generated["expectedEstimateSeconds"].as_u64().unwrap() as u32
                );
                let bytes = build_ctb_container_bytes(&job, &prepared).unwrap();
                for (index, expected) in plan.layers.iter().enumerate() {
                    let actual =
                        read_ctb_layer_settings_from_bytes(&bytes, index as u32 + 1).unwrap();
                    assert!(actual.per_layer_settings, "{name}, layer {}", index + 1);
                    assert_eq!(actual.pwm, expected.pwm, "{name}, layer {} PWM", index + 1);
                    for (value, expected_value) in [
                        (actual.lift_distance_mm, expected.lift_distance_mm),
                        (actual.lift_distance2_mm, expected.lift_distance2_mm),
                        (actual.lift_speed_mm_min, expected.lift_speed_mm_min),
                        (actual.lift_speed2_mm_min, expected.lift_speed2_mm_min),
                        (actual.retract_distance2_mm, expected.retract_distance2_mm),
                        (actual.retract_speed_mm_min, expected.retract_speed_mm_min),
                        (actual.retract_speed2_mm_min, expected.retract_speed2_mm_min),
                        (actual.light_off_delay_sec, expected.light_off_delay_sec),
                        (
                            actual.wait_time_before_cure_sec,
                            expected.wait_time_before_cure_sec,
                        ),
                        (actual.position_z_mm, expected.position_z_mm),
                    ] {
                        assert!(
                            (value - expected_value).abs() < 0.0001,
                            "{name}, layer {}: {value} != {expected_value}",
                            index + 1
                        );
                    }
                }
                std::fs::write(dir.join(format!("{name}.ctb")), bytes).unwrap();
            }
        }
    }
}
