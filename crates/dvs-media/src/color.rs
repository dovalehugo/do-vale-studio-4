//! Color description enums and validated color metadata.

/// Luma/chroma encoding range.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ColorRange {
    /// Studio swing (limited range).
    Limited,
    /// Full-range encoding.
    Full,
    /// Range not specified by the source.
    Unspecified,
}

/// Y′CbCr matrix coefficients.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ColorMatrix {
    /// ITU-R BT.601.
    Bt601,
    /// ITU-R BT.709.
    Bt709,
    /// ITU-R BT.2020 non-constant luminance.
    Bt2020NonConstantLuminance,
    /// Matrix not specified by the source.
    Unspecified,
}

/// Color primaries.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ColorPrimaries {
    /// ITU-R BT.709 primaries.
    Bt709,
    /// ITU-R BT.2020 primaries.
    Bt2020,
    /// Primaries not specified by the source.
    Unspecified,
}

/// Electro-optical transfer characteristic.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum TransferCharacteristic {
    /// ITU-R BT.709 transfer.
    Bt709,
    /// sRGB transfer.
    Srgb,
    /// Perceptual quantizer (PQ / SMPTE ST 2084).
    Pq,
    /// Hybrid log-gamma (HLG).
    Hlg,
    /// Transfer not specified by the source.
    Unspecified,
}

/// Color metadata associated with a video frame.
///
/// Does not assume BT.709 unless explicitly constructed with a named preset.
/// Contains no pixel payload.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct VideoColorInfo {
    range: ColorRange,
    matrix: ColorMatrix,
    primaries: ColorPrimaries,
    transfer: TransferCharacteristic,
}

impl VideoColorInfo {
    /// Creates color metadata from explicit enum values.
    pub const fn new(
        range: ColorRange,
        matrix: ColorMatrix,
        primaries: ColorPrimaries,
        transfer: TransferCharacteristic,
    ) -> Self {
        Self {
            range,
            matrix,
            primaries,
            transfer,
        }
    }

    /// BT.709 limited-range color metadata validated in GPU Experiment 2.
    pub const fn bt709_limited() -> Self {
        Self::new(
            ColorRange::Limited,
            ColorMatrix::Bt709,
            ColorPrimaries::Bt709,
            TransferCharacteristic::Bt709,
        )
    }

    /// Returns the encoded luma/chroma range.
    pub const fn range(self) -> ColorRange {
        self.range
    }

    /// Returns the Y′CbCr matrix coefficients.
    pub const fn matrix(self) -> ColorMatrix {
        self.matrix
    }

    /// Returns the color primaries.
    pub const fn primaries(self) -> ColorPrimaries {
        self.primaries
    }

    /// Returns the transfer characteristic.
    pub const fn transfer(self) -> TransferCharacteristic {
        self.transfer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bt709_limited_constructor_produces_exact_enum_values() {
        let color = VideoColorInfo::bt709_limited();
        assert_eq!(color.range(), ColorRange::Limited);
        assert_eq!(color.matrix(), ColorMatrix::Bt709);
        assert_eq!(color.primaries(), ColorPrimaries::Bt709);
        assert_eq!(color.transfer(), TransferCharacteristic::Bt709);
    }
}
