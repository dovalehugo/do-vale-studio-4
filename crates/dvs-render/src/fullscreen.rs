//! Fullscreen oversized-triangle geometry shared by the WGSL shader and unit tests.

/// Clip-space positions for the three-vertex fullscreen triangle.
pub const CLIP_VERTICES: [[f32; 2]; 3] = [[-1.0, -1.0], [3.0, -1.0], [-1.0, 3.0]];

/// Vertex count for the fullscreen triangle draw call.
pub const DRAW_VERTEX_COUNT: u32 = 3;

/// Maps a clip-space position to the renderer's base UV domain.
///
/// Interpolation over the visible `[-1, 1]` viewport yields `[0, 1] × [0, 1]`.
pub fn base_uv_from_clip(clip: [f32; 2]) -> [f32; 2] {
    [clip[0] * 0.5 + 0.5, -clip[1] * 0.5 + 0.5]
}

/// Returns the base UV emitted for a fullscreen-triangle vertex index.
pub fn base_uv_for_vertex(vertex_index: usize) -> [f32; 2] {
    base_uv_from_clip(CLIP_VERTICES[vertex_index])
}

/// Remaps a base UV through visible crop bounds.
pub fn remapped_uv(base_uv: [f32; 2], uv_min: [f32; 2], uv_max: [f32; 2]) -> [f32; 2] {
    [
        uv_min[0] + (uv_max[0] - uv_min[0]) * base_uv[0],
        uv_min[1] + (uv_max[1] - uv_min[1]) * base_uv[1],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crop::normalized_visible_uv;
    use dvs_media::{
        Extent2D, FrameId, VideoColorInfo, VideoDimensions, VideoFrameMetadata, VideoPixelFormat,
        VisibleRect,
    };

    const TOLERANCE: f32 = 1e-5;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < TOLERANCE
    }

    fn approx_uv(a: [f32; 2], b: [f32; 2]) -> bool {
        approx_eq(a[0], b[0]) && approx_eq(a[1], b[1])
    }

    #[test]
    fn clip_vertices_match_accepted_contract() {
        assert!(approx_uv(CLIP_VERTICES[0], [-1.0, -1.0]));
        assert!(approx_uv(CLIP_VERTICES[1], [3.0, -1.0]));
        assert!(approx_uv(CLIP_VERTICES[2], [-1.0, 3.0]));
    }

    #[test]
    fn vertex_base_uvs_match_clip_mapping() {
        assert!(approx_uv(base_uv_for_vertex(0), [0.0, 1.0]));
        assert!(approx_uv(base_uv_for_vertex(1), [2.0, 1.0]));
        assert!(approx_uv(base_uv_for_vertex(2), [0.0, -1.0]));
    }

    #[test]
    fn viewport_corners_map_to_unit_square() {
        let corners = [
            ([-1.0, -1.0], [0.0, 1.0]),
            ([1.0, -1.0], [1.0, 1.0]),
            ([-1.0, 1.0], [0.0, 0.0]),
            ([1.0, 1.0], [1.0, 0.0]),
        ];
        for (clip, expected) in corners {
            assert!(
                approx_uv(base_uv_from_clip(clip), expected),
                "clip {clip:?} expected {expected:?}"
            );
        }
    }

    #[test]
    fn viewport_center_maps_to_half_half() {
        assert!(approx_uv(base_uv_from_clip([0.0, 0.0]), [0.5, 0.5]));
    }

    #[test]
    fn crop_remapping_does_not_reach_u_one_at_half_viewport_width() {
        let allocation = Extent2D::new(3840, 2176).expect("allocation");
        let visible = VisibleRect::new(0, 0, 3840, 2160).expect("visible");
        let dimensions = VideoDimensions::new(allocation, visible).expect("dimensions");
        let metadata = VideoFrameMetadata::new(
            FrameId::new(0),
            None,
            dimensions,
            VideoPixelFormat::Nv12,
            VideoColorInfo::bt709_limited(),
        );
        let crop = normalized_visible_uv(&metadata).expect("crop");
        let half_width = remapped_uv(base_uv_from_clip([0.0, 0.0]), crop.uv_min, crop.uv_max);
        assert!(half_width[0] < crop.uv_max[0]);
        assert!(approx_eq(half_width[0], 0.5 * crop.uv_max[0]));
        assert!(approx_eq(crop.uv_max[1], 2160.0 / 2176.0));
    }

    #[test]
    fn draw_uses_three_vertices() {
        assert_eq!(DRAW_VERTEX_COUNT, 3);
    }
}
