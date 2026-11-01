//! Application-wide Dock icon; Winit's window icon is unsupported on macOS.
use anyhow::{Context, Result};
use block2::RcBlock;
use objc2::{AnyThread, MainThreadMarker, rc::Retained, runtime::Bool};
use objc2_app_kit::{
    NSApplication, NSBezierPath, NSCompositingOperation, NSGraphicsContext, NSImage,
};
use objc2_foundation::{NSData, NSPoint, NSRect, NSSize};

// The original favicon supplies a 256px image instead of the small window icon.
const DOCK_ICON: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../frontend/public/favicon.ico"
));

const ICON_SIZE: f64 = 256.0;
const ICON_PADDING: f64 = 32.0;
const CORNER_RADIUS: f64 = 20.0;

fn dock_icon() -> Result<Retained<NSImage>> {
    let data = NSData::with_bytes(DOCK_ICON);

    let source =
        NSImage::initWithData(NSImage::alloc(), &data).context("decoding desktop Dock icon")?;
    let draw = RcBlock::new(move |_: NSRect| {
        let bounds = NSRect::new(
            NSPoint::new(ICON_PADDING, ICON_PADDING),
            NSSize::new(
                ICON_SIZE - ICON_PADDING * 2.0,
                ICON_SIZE - ICON_PADDING * 2.0,
            ),
        );
        NSGraphicsContext::saveGraphicsState_class();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            bounds,
            CORNER_RADIUS,
            CORNER_RADIUS,
        )
        .addClip();
        source.drawInRect_fromRect_operation_fraction(
            bounds,
            NSRect::new(NSPoint::new(0.0, 0.0), source.size()),
            NSCompositingOperation::SourceOver,
            1.0,
        );
        NSGraphicsContext::restoreGraphicsState_class();

        Bool::YES
    });

    Ok(NSImage::imageWithSize_flipped_drawingHandler(
        NSSize::new(ICON_SIZE, ICON_SIZE),
        false,
        &draw,
    ))
}

pub(super) fn set_dock_icon() -> Result<()> {
    let main_thread = MainThreadMarker::new().context("Dock icon requires the main thread")?;
    let application = NSApplication::sharedApplication(main_thread);
    let image = dock_icon()?;

    // SAFETY: this is the application's main thread, the image is a valid owned
    // NSImage, and AppKit retains it beyond this call.
    unsafe { application.setApplicationIconImage(Some(&image)) };

    Ok(())
}

#[cfg(test)]
mod tests {
    use objc2::AnyThread;
    use objc2_app_kit::NSBitmapImageRep;

    #[test]
    fn dock_icon_has_padding_and_rounded_transparent_corners() {
        let image = super::dock_icon().unwrap();
        assert!(image.isValid());
        let size = image.size();
        assert_eq!((size.width, size.height), (256.0, 256.0));
        // Render the lazy drawing handler and check actual transparency.
        let data = image.TIFFRepresentation().unwrap();
        let bitmap = NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &data).unwrap();
        for (x, y) in [(0, 0), (23, 128), (128, 23), (24, 24), (231, 231)] {
            assert_eq!(bitmap.colorAtX_y(x, y).unwrap().alphaComponent(), 0.0);
        }
        assert!(bitmap.colorAtX_y(128, 128).unwrap().alphaComponent() > 0.9);
    }
}
