//! Menu-bar icons rendered from SF Symbols, so they match the system's own status items.

use objc2::AnyThread;
use objc2_app_kit::{
    NSBitmapImageRep, NSColor, NSDeviceRGBColorSpace, NSFontWeightRegular, NSGraphicsContext, NSImage,
    NSImageSymbolConfiguration,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use tray_icon::Icon;

/// tray-icon shows icons at most 22 pt tall; we draw at 2× on a 22 pt canvas.
const CANVAS_PT: f64 = 22.0;
const SCALE: f64 = 2.0;
const SYMBOL_PT: f64 = 15.0;

#[derive(Clone)]
pub struct StatusIcon {
    pub icon: Icon,
    /// Template icons are tinted by the system (black/white); colored ones are not.
    pub template: bool,
}

pub struct Icons {
    pub idle: StatusIcon,
    pub unbound: StatusIcon,
    pub recording: StatusIcon,
    pub busy: StatusIcon,
    pub done: StatusIcon,
    pub error: StatusIcon,
    pub download: StatusIcon,
}

impl Icons {
    pub fn load() -> Icons {
        let template = |name| StatusIcon { icon: render(name, false), template: true };
        Icons {
            idle: template("mic"),
            unbound: template("mic.slash"),
            recording: StatusIcon { icon: render("record.circle.fill", true), template: false },
            busy: template("ellipsis.circle"),
            done: template("checkmark.circle"),
            error: template("exclamationmark.triangle"),
            download: template("arrow.down.circle"),
        }
    }
}

fn render(symbol: &str, red: bool) -> Icon {
    draw_symbol(symbol, red).and_then(|(rgba, w, h)| Icon::from_rgba(rgba, w, h).ok()).unwrap_or_else(|| {
        // Fallback: a plain dot, should a symbol ever be unavailable.
        let size = (CANVAS_PT * SCALE) as u32;
        let mut rgba = vec![0u8; (size * size * 4) as usize];
        let c = size as f64 / 2.0;
        for y in 0..size {
            for x in 0..size {
                if ((x as f64 - c).powi(2) + (y as f64 - c).powi(2)).sqrt() < size as f64 / 5.0 {
                    let i = ((y * size + x) * 4) as usize;
                    rgba[i..i + 4].copy_from_slice(&[if red { 255 } else { 0 }, 0, 0, 255]);
                }
            }
        }
        Icon::from_rgba(rgba, size, size).expect("valid fallback icon")
    })
}

/// Straight-alpha RGBA pixels, width and height of `symbol` drawn on the status-item canvas.
fn draw_symbol(symbol: &str, red: bool) -> Option<(Vec<u8>, u32, u32)> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(symbol), None)?;
    let mut config =
        NSImageSymbolConfiguration::configurationWithPointSize_weight(SYMBOL_PT, unsafe { NSFontWeightRegular });
    if red {
        let color = NSImageSymbolConfiguration::configurationWithHierarchicalColor(&NSColor::systemRedColor());
        config = config.configurationByApplyingConfiguration(&color);
    }
    let image = image.imageWithSymbolConfiguration(&config)?;
    let symbol_size = image.size();

    let width_pt = symbol_size.width.ceil().max(1.0);
    let (w, h) = ((width_pt * SCALE) as isize, (CANVAS_PT * SCALE) as isize);
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            w,
            h,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            w * 4,
            32,
        )
    }?;
    rep.setSize(NSSize::new(width_pt, CANVAS_PT));
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    let origin = NSPoint::new((width_pt - symbol_size.width) / 2.0, (CANVAS_PT - symbol_size.height) / 2.0);
    image.drawInRect(NSRect::new(origin, symbol_size));
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();

    let stride = rep.bytesPerRow() as usize;
    let data = rep.bitmapData();
    if data.is_null() {
        return None;
    }
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        let row = unsafe { std::slice::from_raw_parts(data.add(y * stride), w as usize * 4) };
        for px in row.as_chunks::<4>().0 {
            // The bitmap is premultiplied; tray-icon expects straight alpha.
            let a = px[3];
            let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
            rgba.extend_from_slice(&[un(px[0]), un(px[1]), un(px[2]), a]);
        }
    }
    Some((rgba, w as u32, h as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_every_status_symbol() {
        let symbols = [
            ("mic", false),
            ("mic.slash", false),
            ("record.circle.fill", true),
            ("ellipsis.circle", false),
            ("checkmark.circle", false),
            ("exclamationmark.triangle", false),
            ("arrow.down.circle", false),
        ];
        for (name, red) in symbols {
            let (rgba, w, h) = draw_symbol(name, red).unwrap_or_else(|| panic!("{name} did not render"));
            assert_eq!(h, 44, "{name}");
            assert!((20..=60).contains(&w), "{name}: width {w}");
            let opaque = rgba.chunks(4).filter(|px| px[3] > 128).count();
            assert!(opaque > 60, "{name}: only {opaque} opaque pixels");
            let reddish = rgba.chunks(4).filter(|px| px[3] > 128 && px[0] > 180 && px[1] < 120).count();
            assert_eq!(reddish > opaque / 2, red, "{name}: color");
            // EXPLAINIT_ICON_DUMP=<dir> writes the raw pixels for visual inspection.
            if let Some(dir) = std::env::var_os("EXPLAINIT_ICON_DUMP") {
                std::fs::write(std::path::Path::new(&dir).join(format!("{name}-{w}x{h}.rgba")), &rgba).unwrap();
            }
        }
    }
}
