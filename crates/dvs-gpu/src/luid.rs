//! Platform-independent DXGI adapter LUID representation and matching.

use std::fmt;

use crate::error::GpuError;

/// Exact DXGI adapter locally unique identifier (LUID).
///
/// Identifies the physical GPU adapter. Equality — not numeric ordering — determines
/// whether two LUID values refer to the same adapter. Vendor and device IDs must not
/// be used as a substitute.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DxgiAdapterLuid {
    low_part: u32,
    high_part: i32,
}

impl DxgiAdapterLuid {
    /// Creates a LUID from DXGI `LowPart` and `HighPart` values.
    pub const fn new(low_part: u32, high_part: i32) -> Self {
        Self {
            low_part,
            high_part,
        }
    }

    /// Returns the DXGI `LowPart`.
    pub const fn low_part(self) -> u32 {
        self.low_part
    }

    /// Returns the DXGI `HighPart`.
    pub const fn high_part(self) -> i32 {
        self.high_part
    }

    /// Returns the 64-bit bit pattern formed from low and high parts.
    ///
    /// The high part is sign-extended into the upper 32 bits, preserving the exact
    /// two's-complement DXGI representation.
    pub const fn as_u64_bits(self) -> u64 {
        ((self.high_part as i64 as u64) << 32) | self.low_part as u64
    }
}

impl fmt::Display for DxgiAdapterLuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08X}-{:08X}", self.high_part as u32, self.low_part)
    }
}

/// Validates that two LUID values identify the same physical adapter.
pub fn validate_same_adapter(
    expected: DxgiAdapterLuid,
    actual: DxgiAdapterLuid,
) -> Result<(), GpuError> {
    if expected == actual {
        Ok(())
    } else {
        Err(GpuError::AdapterLuidMismatch { expected, actual })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luid_constructor_and_accessors() {
        let luid = DxgiAdapterLuid::new(0x00A2_B3C4, 0x0001_0000);
        assert_eq!(luid.low_part(), 0x00A2_B3C4);
        assert_eq!(luid.high_part(), 0x0001_0000);
    }

    #[test]
    fn as_u64_bits_with_zero_high_part() {
        let luid = DxgiAdapterLuid::new(0x1234_5678, 0);
        assert_eq!(luid.as_u64_bits(), 0x1234_5678);
    }

    #[test]
    fn as_u64_bits_preserves_negative_high_part_bit_pattern() {
        let luid = DxgiAdapterLuid::new(0, -1);
        assert_eq!(luid.as_u64_bits(), 0xFFFF_FFFF_0000_0000);
    }

    #[test]
    fn display_uses_fixed_width_hexadecimal() {
        let luid = DxgiAdapterLuid::new(0x00A2_B3C4, 0x0001_0000);
        assert_eq!(format!("{luid}"), "00010000-00A2B3C4");
    }

    #[test]
    fn equal_luids_validate_successfully() {
        let expected = DxgiAdapterLuid::new(1, 2);
        validate_same_adapter(expected, expected).expect("same LUID");
    }

    #[test]
    fn different_low_parts_produce_adapter_luid_mismatch() {
        let expected = DxgiAdapterLuid::new(1, 2);
        let actual = DxgiAdapterLuid::new(3, 2);
        match validate_same_adapter(expected, actual).unwrap_err() {
            GpuError::AdapterLuidMismatch {
                expected: e,
                actual: a,
            } => {
                assert_eq!(e, expected);
                assert_eq!(a.low_part(), 3);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn different_high_parts_produce_adapter_luid_mismatch() {
        let expected = DxgiAdapterLuid::new(1, 2);
        let actual = DxgiAdapterLuid::new(1, 4);
        match validate_same_adapter(expected, actual).unwrap_err() {
            GpuError::AdapterLuidMismatch {
                expected: e,
                actual: a,
            } => {
                assert_eq!(e, expected);
                assert_eq!(a.high_part(), 4);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn adapter_luid_mismatch_display_contains_expected_and_actual_values() {
        let expected = DxgiAdapterLuid::new(0x1111_1111, 0x0000_0001);
        let actual = DxgiAdapterLuid::new(0x2222_2222, 0x0000_0002);
        let message = validate_same_adapter(expected, actual)
            .unwrap_err()
            .to_string();
        assert!(message.contains("00000001-11111111"));
        assert!(message.contains("00000002-22222222"));
    }
}
