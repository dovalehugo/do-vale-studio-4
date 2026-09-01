//! Allocation and visible-crop dimension types.

use crate::error::MetadataError;

/// A non-zero width and height pair describing a surface allocation extent.
///
/// Allocation dimensions may include decoder alignment padding beyond the
/// displayable picture. They do not imply any pixel storage or buffer layout.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct Extent2D {
    width: u32,
    height: u32,
}

impl Extent2D {
    /// Creates an extent with validated non-zero width and height.
    pub fn new(width: u32, height: u32) -> Result<Self, MetadataError> {
        if width == 0 {
            return Err(MetadataError::ZeroWidth);
        }
        if height == 0 {
            return Err(MetadataError::ZeroHeight);
        }
        Ok(Self { width, height })
    }

    /// Returns the allocation width in pixels.
    pub fn width(self) -> u32 {
        self.width
    }

    /// Returns the allocation height in pixels.
    pub fn height(self) -> u32 {
        self.height
    }
}

/// A validated visible crop rectangle within an allocation.
///
/// Width and height must be non-zero. Coordinate addition is checked for overflow.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct VisibleRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl VisibleRect {
    /// Creates a visible rectangle with validated non-zero size and checked coordinates.
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Result<Self, MetadataError> {
        if width == 0 {
            return Err(MetadataError::ZeroWidth);
        }
        if height == 0 {
            return Err(MetadataError::ZeroHeight);
        }
        let right = x
            .checked_add(width)
            .ok_or(MetadataError::DimensionOverflow)?;
        let bottom = y
            .checked_add(height)
            .ok_or(MetadataError::DimensionOverflow)?;
        // `right` and `bottom` are computed only to validate overflow; values are not stored.
        let _ = (right, bottom);
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }

    /// Returns the horizontal offset of the visible crop from the allocation origin.
    pub fn x(self) -> u32 {
        self.x
    }

    /// Returns the vertical offset of the visible crop from the allocation origin.
    pub fn y(self) -> u32 {
        self.y
    }

    /// Returns the visible width in pixels.
    pub fn width(self) -> u32 {
        self.width
    }

    /// Returns the visible height in pixels.
    pub fn height(self) -> u32 {
        self.height
    }
}

/// Allocation extent together with the displayable visible crop.
///
/// The visible rectangle must fit entirely inside the allocation. Allocation
/// dimensions may be larger than the visible picture (for example, when a
/// decoder pads height for alignment). Visible dimensions describe the
/// displayable crop only.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct VideoDimensions {
    allocation: Extent2D,
    visible: VisibleRect,
}

impl VideoDimensions {
    /// Creates dimensions with a visible crop validated against the allocation extent.
    pub fn new(allocation: Extent2D, visible: VisibleRect) -> Result<Self, MetadataError> {
        let alloc_w = allocation.width();
        let alloc_h = allocation.height();

        let visible_right = visible
            .x()
            .checked_add(visible.width())
            .ok_or(MetadataError::DimensionOverflow)?;
        let visible_bottom = visible
            .y()
            .checked_add(visible.height())
            .ok_or(MetadataError::DimensionOverflow)?;

        if visible_right > alloc_w || visible_bottom > alloc_h {
            return Err(MetadataError::VisibleRectOutOfBounds);
        }

        Ok(Self {
            allocation,
            visible,
        })
    }

    /// Returns the full allocation extent (may include decoder padding).
    pub fn allocation(self) -> Extent2D {
        self.allocation
    }

    /// Returns the displayable visible crop within the allocation.
    pub fn visible(self) -> VisibleRect {
        self.visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn experiment_2_dimensions_accepted() {
        let allocation = Extent2D::new(3840, 2176).expect("allocation");
        let visible = VisibleRect::new(0, 0, 3840, 2160).expect("visible");
        let dims = VideoDimensions::new(allocation, visible).expect("dimensions");
        assert_eq!(dims.allocation().width(), 3840);
        assert_eq!(dims.allocation().height(), 2176);
        assert_eq!(dims.visible().width(), 3840);
        assert_eq!(dims.visible().height(), 2160);
    }

    #[test]
    fn visible_rect_equal_to_allocation_accepted() {
        let allocation = Extent2D::new(1920, 1080).expect("allocation");
        let visible = VisibleRect::new(0, 0, 1920, 1080).expect("visible");
        VideoDimensions::new(allocation, visible).expect("dimensions");
    }

    #[test]
    fn zero_allocation_width_rejected() {
        let err = Extent2D::new(0, 1080).unwrap_err();
        assert_eq!(err, MetadataError::ZeroWidth);
    }

    #[test]
    fn zero_visible_height_rejected() {
        let err = VisibleRect::new(0, 0, 1920, 0).unwrap_err();
        assert_eq!(err, MetadataError::ZeroHeight);
    }

    #[test]
    fn visible_rect_exceeding_allocation_width_rejected() {
        let allocation = Extent2D::new(1920, 1080).expect("allocation");
        let visible = VisibleRect::new(1, 0, 1920, 1080).expect("visible");
        let err = VideoDimensions::new(allocation, visible).unwrap_err();
        assert_eq!(err, MetadataError::VisibleRectOutOfBounds);
    }

    #[test]
    fn visible_rect_exceeding_allocation_height_rejected() {
        let allocation = Extent2D::new(3840, 2176).expect("allocation");
        let visible = VisibleRect::new(0, 0, 3840, 2177).expect("visible");
        let err = VideoDimensions::new(allocation, visible).unwrap_err();
        assert_eq!(err, MetadataError::VisibleRectOutOfBounds);
    }

    #[test]
    fn coordinate_addition_overflow_rejected() {
        let err = VisibleRect::new(u32::MAX, 0, 1, 1).unwrap_err();
        assert_eq!(err, MetadataError::DimensionOverflow);
    }
}
