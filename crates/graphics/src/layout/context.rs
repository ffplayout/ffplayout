//! Explicit pixel coordinate spaces; no encoder or output protocol dependencies.
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelSizing {
    /// Stretch from the reference canvas into the destination, including its
    /// aspect ratio. Program aspect/fit correction happens before composition.
    ReferenceCanvas,
    /// Keep prepared dimensions in destination pixels (legacy program logos).
    Destination,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutContext {
    reference: (u32, u32),
    destination: (u32, u32),
}

impl LayoutContext {
    pub fn new(reference: (u32, u32), destination: (u32, u32)) -> Result<Self> {
        ensure!(
            reference.0 > 0 && reference.1 > 0 && destination.0 > 0 && destination.1 > 0,
            "layout canvases must be positive"
        );

        Ok(Self {
            reference,
            destination,
        })
    }

    /// Surface extent is aligned down to even pixels, with a two-pixel minimum.
    /// Positions align towards negative infinity after integer scaling. Negative
    /// positions remain valid; the compositor clips to destination boundaries.
    pub fn yuva_geometry(
        self,
        size: (u32, u32),
        position: (i32, i32),
        sizing: PixelSizing,
    ) -> Result<((u32, u32), (i32, i32))> {
        let (reference, destination) = match sizing {
            PixelSizing::ReferenceCanvas => (self.reference, self.destination),
            PixelSizing::Destination => ((1, 1), (1, 1)),
        };
        let width = u64::from(size.0) * u64::from(destination.0) / u64::from(reference.0);
        let height = u64::from(size.1) * u64::from(destination.1) / u64::from(reference.1);
        let x = i64::from(position.0) * i64::from(destination.0) / i64::from(reference.0);
        let y = i64::from(position.1) * i64::from(destination.1) / i64::from(reference.1);
        ensure!(
            width <= u64::from(u32::MAX) && height <= u64::from(u32::MAX),
            "scaled surface dimensions overflow"
        );
        let x = i32::try_from(x)? & !1;
        let y = i32::try_from(y)? & !1;

        Ok((
            ((width.max(2) as u32) & !1, (height.max(2) as u32) & !1),
            (x, y),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_and_destination_pixels_are_explicit_and_negative_positions_clip() {
        let layout = LayoutContext::new((1280, 720), (640, 480)).unwrap();
        assert_eq!(
            layout
                .yuva_geometry((200, 60), (-7, 9), PixelSizing::ReferenceCanvas)
                .unwrap(),
            ((100, 40), (-4, 6))
        );
        assert_eq!(
            layout
                .yuva_geometry((200, 60), (-7, 9), PixelSizing::Destination)
                .unwrap(),
            ((200, 60), (-8, 8))
        );
        assert!(LayoutContext::new((0, 720), (640, 480)).is_err());
        let huge = LayoutContext::new((1, 1), (u32::MAX, 2)).unwrap();
        assert!(
            huge.yuva_geometry((4, 4), (0, 0), PixelSizing::ReferenceCanvas)
                .is_err()
        );
        assert!(
            huge.yuva_geometry((1, 1), (i32::MAX, 0), PixelSizing::ReferenceCanvas)
                .is_err()
        );
    }
}
