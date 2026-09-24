//! Reads stored CTB v4/v5 records, never regenerates settings from a profile.
use super::ctb_types::*;
use serde::Serialize;
use std::{
    io::{Cursor, Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CtbStoredLayerSettings {
    pub layer_number: u32,
    pub layer_count: u32,
    pub version: u32,
    pub bottom_layer_count: u32,
    pub per_layer_settings: bool,
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
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}
fn f32_at(data: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}
fn read_at(reader: &mut (impl Read + Seek), offset: u64, size: usize) -> Result<Vec<u8>, String> {
    let length = reader.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
    if offset
        .checked_add(size as u64)
        .filter(|end| *end <= length)
        .is_none()
    {
        return Err("CTB record lies outside the file".into());
    }
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut result = vec![0; size];
    reader.read_exact(&mut result).map_err(|e| e.to_string())?;
    Ok(result)
}

pub fn read_ctb_layer_settings_from_file(
    path: &Path,
    layer_number: u32,
) -> Result<CtbStoredLayerSettings, String> {
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("Failed opening CTB file: {e}"))?;
    read_settings(&mut file, layer_number)
}

pub fn read_ctb_layer_settings_from_bytes(
    bytes: &[u8],
    layer_number: u32,
) -> Result<CtbStoredLayerSettings, String> {
    read_settings(&mut Cursor::new(bytes), layer_number)
}

fn read_settings(
    reader: &mut (impl Read + Seek),
    layer_number: u32,
) -> Result<CtbStoredLayerSettings, String> {
    if layer_number == 0 {
        return Err("Layer number must be >= 1".into());
    }
    let header = read_at(reader, 0, CTB_HEADER_SIZE as usize)?;
    let magic = u32_at(&header, 0);
    let encrypted = magic == CTB_MAGIC_V5_ENCRYPTED;
    if !encrypted && magic != CTB_MAGIC_V4_V5 {
        return Err("Stored timing inspector requires CTB v4 or v5".into());
    }
    let version = u32_at(&header, if encrypted { 16 } else { 4 });
    if !(4..=5).contains(&version) {
        return Err("Stored timing inspector requires CTB v4 or v5".into());
    }
    let (layer_count, bottom_layer_count, per_layer_settings, table_offset) = if encrypted {
        let mut settings = read_at(
            reader,
            CTB_ENCRYPTED_SETTINGS_OFFSET as u64,
            CTB_ENCRYPTED_SETTINGS_SIZE as usize,
        )?;
        let (key, iv) = super::ctb_crypto::ctb_default_key_iv();
        super::ctb_crypto::ctb_decrypt_in_place_no_padding(&mut settings, &key, &iv)
            .map_err(|e| e.to_string())?;
        (
            u32_at(&settings, 64),
            u32_at(&settings, 52),
            settings[171] != 0,
            u32_at(&settings, 8),
        )
    } else {
        let slicer = read_at(
            reader,
            u32_at(&header, 104) as u64,
            CTB_SLICER_INFO_FIXED_SIZE as usize,
        )?;
        (
            u32_at(&header, 68),
            u32_at(&header, 48),
            slicer[39] != 0,
            u32_at(&header, 64),
        )
    };
    if layer_number > layer_count {
        return Err(format!(
            "Layer {layer_number} out of range (file has {layer_count} layers)"
        ));
    }
    let record = if encrypted {
        let ptr = read_at(
            reader,
            table_offset as u64 + (layer_number - 1) as u64 * 16,
            16,
        )?;
        if u32_at(&ptr, 8) != CTB_ENCRYPTED_LAYER_DEF_SIZE {
            return Err("Unsupported encrypted CTB layer record size".into());
        }
        read_at(
            reader,
            u32_at(&ptr, 4) as u64 * CTB_PAGE_SIZE + u32_at(&ptr, 0) as u64,
            CTB_ENCRYPTED_LAYER_DEF_SIZE as usize,
        )?
    } else {
        let base = read_at(
            reader,
            table_offset as u64 + (layer_number - 1) as u64 * CTB_LAYER_DEF_SIZE as u64,
            CTB_LAYER_DEF_SIZE as usize,
        )?;
        if u32_at(&base, 24) != CTB_LAYER_DEF_EX_SIZE {
            return Err("CTB layer does not contain extended timing fields".into());
        }
        let data_offset = u32_at(&base, 20) as u64 * CTB_PAGE_SIZE + u32_at(&base, 12) as u64;
        let offset = data_offset
            .checked_sub(CTB_LAYER_DEF_EX_SIZE as u64)
            .ok_or("Invalid CTB extended layer offset")?;
        let extended = read_at(reader, offset, CTB_LAYER_DEF_EX_SIZE as usize)?;
        if extended[..36] != base[..] {
            return Err("CTB base and extended layer records disagree".into());
        }
        extended
    };
    let shift = usize::from(encrypted) * 4;
    let numeric_offsets = [
        shift,
        4 + shift,
        8 + shift,
        40,
        44,
        48,
        52,
        56,
        60,
        64,
        68,
        72,
        76,
        80,
    ];
    if numeric_offsets.iter().any(|offset| {
        let n = f32_at(&record, *offset);
        !n.is_finite() || n < 0.0
    }) {
        return Err("CTB layer contains invalid timing values".into());
    }
    Ok(CtbStoredLayerSettings {
        layer_number,
        layer_count,
        version,
        bottom_layer_count,
        per_layer_settings,
        position_z_mm: f32_at(&record, shift),
        exposure_sec: f32_at(&record, 4 + shift),
        light_off_delay_sec: f32_at(&record, 8 + shift),
        lift_distance_mm: (f32_at(&record, 40) - f32_at(&record, 48)).max(0.0),
        lift_distance2_mm: f32_at(&record, 48),
        lift_speed_mm_min: f32_at(&record, 44),
        lift_speed2_mm_min: f32_at(&record, 52),
        retract_speed_mm_min: f32_at(&record, 56),
        retract_distance2_mm: f32_at(&record, 60),
        retract_speed2_mm_min: f32_at(&record, 64),
        wait_time_after_cure_sec: f32_at(&record, 68),
        wait_time_after_lift_sec: f32_at(&record, 72),
        wait_time_before_cure_sec: f32_at(&record, 76),
        pwm: f32_at(&record, 80) as u16,
    })
}
