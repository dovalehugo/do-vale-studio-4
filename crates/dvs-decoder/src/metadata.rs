//! Pure metadata mapping helpers (FFmpeg values → `dvs-media` types).

use dvs_media::{
    ColorMatrix, ColorPrimaries, ColorRange, Extent2D, FrameId, MediaTimestamp, TimeBase,
    TransferCharacteristic, VideoColorInfo, VideoDimensions, VideoFrameMetadata, VideoPixelFormat,
    VisibleRect,
};

use crate::error::DecoderError;

/// FFmpeg `AV_NOPTS_VALUE`.
pub const AV_NOPTS_VALUE: i64 = 0x8000_0000_0000_0000u64 as i64;

/// FFmpeg `AVCOL_RANGE_MPEG` (limited/studio swing).
pub const AVCOL_RANGE_MPEG: i32 = 1;
/// FFmpeg `AVCOL_RANGE_JPEG` (full range).
pub const AVCOL_RANGE_JPEG: i32 = 2;

/// FFmpeg `AVCOL_SPC_BT709`.
pub const AVCOL_SPC_BT709: i32 = 1;
/// FFmpeg `AVCOL_SPC_BT470BG`.
pub const AVCOL_SPC_BT470BG: i32 = 5;
/// FFmpeg `AVCOL_SPC_SMPTE170M`.
pub const AVCOL_SPC_SMPTE170M: i32 = 6;
/// FFmpeg `AVCOL_SPC_BT2020_NCL`.
pub const AVCOL_SPC_BT2020_NCL: i32 = 9;

/// FFmpeg `AVCOL_PRI_BT709`.
pub const AVCOL_PRI_BT709: i32 = 1;
/// FFmpeg `AVCOL_PRI_BT2020`.
pub const AVCOL_PRI_BT2020: i32 = 9;

/// FFmpeg `AVCOL_TRC_BT709`.
pub const AVCOL_TRC_BT709: i32 = 1;
/// FFmpeg `AVCOL_TRC_IEC61966_2_1` (sRGB).
pub const AVCOL_TRC_IEC61966_2_1: i32 = 13;
/// FFmpeg `AVCOL_TRC_SMPTE2084` (PQ).
pub const AVCOL_TRC_SMPTE2084: i32 = 16;
/// FFmpeg `AVCOL_TRC_ARIB_STD_B67` (HLG).
pub const AVCOL_TRC_ARIB_STD_B67: i32 = 18;

/// Maps FFmpeg `AVColorRange` to `dvs-media` `ColorRange`.
pub fn map_color_range(value: i32) -> ColorRange {
    match value {
        AVCOL_RANGE_MPEG => ColorRange::Limited,
        AVCOL_RANGE_JPEG => ColorRange::Full,
        _ => ColorRange::Unspecified,
    }
}

/// Maps FFmpeg `AVColorSpace` to `dvs-media` `ColorMatrix`.
pub fn map_color_matrix(value: i32) -> ColorMatrix {
    match value {
        AVCOL_SPC_BT709 => ColorMatrix::Bt709,
        AVCOL_SPC_BT470BG | AVCOL_SPC_SMPTE170M => ColorMatrix::Bt601,
        AVCOL_SPC_BT2020_NCL => ColorMatrix::Bt2020NonConstantLuminance,
        _ => ColorMatrix::Unspecified,
    }
}

/// Maps FFmpeg `AVColorPrimaries` to `dvs-media` `ColorPrimaries`.
pub fn map_color_primaries(value: i32) -> ColorPrimaries {
    match value {
        AVCOL_PRI_BT709 => ColorPrimaries::Bt709,
        AVCOL_PRI_BT2020 => ColorPrimaries::Bt2020,
        _ => ColorPrimaries::Unspecified,
    }
}

/// Maps FFmpeg `AVColorTransferCharacteristic` to `dvs-media` `TransferCharacteristic`.
pub fn map_transfer_characteristic(value: i32) -> TransferCharacteristic {
    match value {
        AVCOL_TRC_BT709 => TransferCharacteristic::Bt709,
        AVCOL_TRC_IEC61966_2_1 => TransferCharacteristic::Srgb,
        AVCOL_TRC_SMPTE2084 => TransferCharacteristic::Pq,
        AVCOL_TRC_ARIB_STD_B67 => TransferCharacteristic::Hlg,
        _ => TransferCharacteristic::Unspecified,
    }
}

/// Builds `VideoColorInfo` from FFmpeg frame color fields.
pub fn color_info_from_ffmpeg(
    color_range: i32,
    colorspace: i32,
    color_primaries: i32,
    color_trc: i32,
) -> VideoColorInfo {
    VideoColorInfo::new(
        map_color_range(color_range),
        map_color_matrix(colorspace),
        map_color_primaries(color_primaries),
        map_transfer_characteristic(color_trc),
    )
}

/// Converts FFmpeg PTS and stream time base to an optional `MediaTimestamp`.
pub fn pts_to_timestamp(
    pts: i64,
    time_base_num: i32,
    time_base_den: i32,
) -> Result<Option<MediaTimestamp>, DecoderError> {
    if pts == AV_NOPTS_VALUE {
        return Ok(None);
    }
    if time_base_num <= 0 || time_base_den <= 0 {
        return Err(DecoderError::InvalidDecoderState {
            detail: "stream time base is invalid",
        });
    }
    let time_base = TimeBase::new(time_base_num as u32, time_base_den as u32)?;
    Ok(Some(MediaTimestamp::new(pts, time_base)))
}

/// Builds validated allocation and visible dimensions.
pub fn build_dimensions(
    allocation_width: u32,
    allocation_height: u32,
    visible_x: u32,
    visible_y: u32,
    visible_width: u32,
    visible_height: u32,
) -> Result<VideoDimensions, DecoderError> {
    let allocation = Extent2D::new(allocation_width, allocation_height)?;
    let visible = VisibleRect::new(visible_x, visible_y, visible_width, visible_height)?;
    Ok(VideoDimensions::new(allocation, visible)?)
}

/// Builds `VideoFrameMetadata` for a decoded D3D11 frame.
#[allow(clippy::too_many_arguments)]
pub fn build_frame_metadata(
    frame_id: FrameId,
    pts: i64,
    time_base_num: i32,
    time_base_den: i32,
    allocation_width: u32,
    allocation_height: u32,
    visible_x: u32,
    visible_y: u32,
    visible_width: u32,
    visible_height: u32,
    color_range: i32,
    colorspace: i32,
    color_primaries: i32,
    color_trc: i32,
) -> Result<VideoFrameMetadata, DecoderError> {
    let timestamp = pts_to_timestamp(pts, time_base_num, time_base_den)?;
    let dimensions = build_dimensions(
        allocation_width,
        allocation_height,
        visible_x,
        visible_y,
        visible_width,
        visible_height,
    )?;
    let color = color_info_from_ffmpeg(color_range, colorspace, color_primaries, color_trc);
    Ok(VideoFrameMetadata::new(
        frame_id,
        timestamp,
        dimensions,
        VideoPixelFormat::Nv12,
        color,
    ))
}

/// Returns the monotonic frame identifier for the given counter.
pub fn next_frame_id(current: u64) -> FrameId {
    FrameId::new(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dvs_gpu::DxgiAdapterLuid;

    #[test]
    fn color_range_mapping() {
        assert_eq!(map_color_range(AVCOL_RANGE_MPEG), ColorRange::Limited);
        assert_eq!(map_color_range(AVCOL_RANGE_JPEG), ColorRange::Full);
        assert_eq!(map_color_range(0), ColorRange::Unspecified);
    }

    #[test]
    fn color_matrix_mapping() {
        assert_eq!(map_color_matrix(AVCOL_SPC_BT709), ColorMatrix::Bt709);
        assert_eq!(map_color_matrix(AVCOL_SPC_BT470BG), ColorMatrix::Bt601);
        assert_eq!(map_color_matrix(-1), ColorMatrix::Unspecified);
    }

    #[test]
    fn transfer_characteristic_mapping() {
        assert_eq!(
            map_transfer_characteristic(AVCOL_TRC_SMPTE2084),
            TransferCharacteristic::Pq
        );
        assert_eq!(
            map_transfer_characteristic(99),
            TransferCharacteristic::Unspecified
        );
    }

    #[test]
    fn absent_pts_maps_to_none() {
        let ts = pts_to_timestamp(AV_NOPTS_VALUE, 1, 60_000).expect("pts");
        assert!(ts.is_none());
    }

    #[test]
    fn present_pts_maps_to_timestamp() {
        let ts = pts_to_timestamp(3_600_000, 1, 60_000)
            .expect("pts")
            .expect("some");
        assert_eq!(ts.pts(), 3_600_000);
        assert_eq!(ts.time_base().numerator(), 1);
        assert_eq!(ts.time_base().denominator(), 60_000);
    }

    #[test]
    fn invalid_time_base_rejected() {
        let err = pts_to_timestamp(1, 0, 60_000).unwrap_err();
        assert!(matches!(err, DecoderError::InvalidDecoderState { .. }));
    }

    #[test]
    fn experiment_fixture_dimensions_validate() {
        let dims = build_dimensions(3840, 2176, 0, 0, 3840, 2160).expect("dims");
        assert_eq!(dims.allocation().width(), 3840);
        assert_eq!(dims.allocation().height(), 2176);
        assert_eq!(dims.visible().height(), 2160);
    }

    #[test]
    fn frame_id_progression() {
        assert_eq!(next_frame_id(0).value(), 0);
        assert_eq!(next_frame_id(4).value(), 4);
    }

    #[test]
    fn adapter_mismatch_error_fields() {
        let expected = DxgiAdapterLuid::new(1, 2);
        let actual = DxgiAdapterLuid::new(3, 4);
        let err = DecoderError::AdapterLuidMismatch { expected, actual };
        let message = err.to_string();
        assert!(message.contains("expected"));
        assert!(message.contains("actual"));
    }
}
