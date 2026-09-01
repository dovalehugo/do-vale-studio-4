//! NV12 allocation dimension validation and D3D11 subresource helpers (platform-independent).

use crate::error::GpuError;

/// Validates NV12 allocation dimensions.
pub(crate) fn validate_nv12_allocation_dimensions(width: u32, height: u32) -> Result<(), GpuError> {
    if width == 0 || height == 0 {
        if width == 0 {
            return Err(GpuError::InvalidSharedTextureDimensions);
        }
        return Err(GpuError::InvalidSharedTextureDimensions);
    }
    if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return Err(GpuError::Nv12DimensionsMustBeEven);
    }
    Ok(())
}

/// Calculates the D3D11 subresource index for mip 0.
pub(crate) fn d3d11_subresource_index(array_slice: u32, mip_levels: u32) -> Result<u32, GpuError> {
    if mip_levels == 0 {
        return Err(GpuError::DecoderTextureUnsupportedLayout);
    }
    Ok(array_slice * mip_levels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_zero_width() {
        assert!(matches!(
            validate_nv12_allocation_dimensions(0, 1080),
            Err(GpuError::InvalidSharedTextureDimensions)
        ));
    }

    #[test]
    fn reject_zero_height() {
        assert!(matches!(
            validate_nv12_allocation_dimensions(1920, 0),
            Err(GpuError::InvalidSharedTextureDimensions)
        ));
    }

    #[test]
    fn reject_odd_nv12_width() {
        assert!(matches!(
            validate_nv12_allocation_dimensions(3841, 2176),
            Err(GpuError::Nv12DimensionsMustBeEven)
        ));
    }

    #[test]
    fn reject_odd_nv12_height() {
        assert!(matches!(
            validate_nv12_allocation_dimensions(3840, 2177),
            Err(GpuError::Nv12DimensionsMustBeEven)
        ));
    }

    #[test]
    fn accept_3840_by_2176() {
        validate_nv12_allocation_dimensions(3840, 2176).expect("valid");
    }

    #[test]
    fn subresource_for_array_slice_zero() {
        assert_eq!(d3d11_subresource_index(0, 1).expect("subresource"), 0);
    }

    #[test]
    fn subresource_for_non_zero_array_slice() {
        assert_eq!(d3d11_subresource_index(2, 4).expect("subresource"), 8);
    }

    #[test]
    fn subresource_for_array_slice_with_multiple_mip_levels() {
        assert_eq!(d3d11_subresource_index(1, 2).expect("subresource"), 2);
    }
}
