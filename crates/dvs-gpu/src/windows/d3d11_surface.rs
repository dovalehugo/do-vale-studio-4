//! Borrowed D3D11 decoder surfaces and shareable NV12 allocation descriptors.

#![allow(unsafe_code)]

use windows::Win32::Graphics::Direct3D11::{D3D11_TEXTURE2D_DESC, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;

use crate::error::GpuError;
use crate::nv12_allocation::{d3d11_subresource_index, validate_nv12_allocation_dimensions};

/// Borrowed D3D11 decoder surface used as a GPU copy source.
///
/// `dvs-decoder` constructs this transiently from a private `AVFrame`. The reference
/// must not outlive the decoder-owned texture. This type performs no GPU work in its
/// constructor and is intentionally not `Send` or `Sync`.
#[derive(Debug)]
pub struct D3d11DecodedSurfaceRef<'a> {
    texture: &'a ID3D11Texture2D,
    array_slice: u32,
}

impl<'a> D3d11DecodedSurfaceRef<'a> {
    /// Creates a borrowed decoder surface reference without issuing GPU commands.
    pub fn new(texture: &'a ID3D11Texture2D, array_slice: u32) -> Result<Self, GpuError> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `texture` is a live COM object borrowed from the decoder for metadata only.
        unsafe {
            texture.GetDesc(&mut desc);
        }
        if array_slice >= desc.ArraySize {
            return Err(GpuError::DecoderTextureArraySliceOutOfBounds);
        }
        Ok(Self {
            texture,
            array_slice,
        })
    }

    pub(crate) fn texture(&self) -> &ID3D11Texture2D {
        self.texture
    }

    pub(crate) fn validate_for_copy(
        &self,
        allocation_width: u32,
        allocation_height: u32,
    ) -> Result<u32, GpuError> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `texture` remains borrowed and valid for descriptor inspection only.
        unsafe {
            self.texture.GetDesc(&mut desc);
        }

        if desc.Format != DXGI_FORMAT_NV12 {
            return Err(GpuError::InvalidDecoderTextureFormat);
        }
        if desc.Width != allocation_width || desc.Height != allocation_height {
            return Err(GpuError::DecoderTextureDimensionsMismatch);
        }
        if desc.MipLevels != 1 || desc.SampleDesc.Count != 1 {
            return Err(GpuError::DecoderTextureUnsupportedLayout);
        }
        if self.array_slice >= desc.ArraySize {
            return Err(GpuError::DecoderTextureArraySliceOutOfBounds);
        }

        d3d11_subresource_index(self.array_slice, desc.MipLevels)
    }
}

/// Allocation dimensions for the single shared NV12 producer texture.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct SharedNv12TextureDesc {
    allocation_width: u32,
    allocation_height: u32,
}

impl SharedNv12TextureDesc {
    /// Creates validated NV12 allocation dimensions.
    pub fn new(allocation_width: u32, allocation_height: u32) -> Result<Self, GpuError> {
        validate_nv12_allocation_dimensions(allocation_width, allocation_height)?;
        Ok(Self {
            allocation_width,
            allocation_height,
        })
    }

    /// Returns the allocation width in pixels.
    pub fn allocation_width(self) -> u32 {
        self.allocation_width
    }

    /// Returns the allocation height in pixels.
    pub fn allocation_height(self) -> u32 {
        self.allocation_height
    }
}
