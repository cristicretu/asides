//! Reading images from (and writing text to) the general pasteboard.

use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypePNG,
    NSPasteboardTypeString,
};
use objc2_foundation::{NSData, NSDictionary, NSString, NSURL};

/// Kept as-is when copied from Finder; anything else is converted to PNG.
const PORTABLE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];
const CONVERTIBLE_EXTENSIONS: &[&str] = &["heic", "heif", "tif", "tiff", "bmp"];

pub struct ClipboardImage {
    pub bytes: Vec<u8>,
    pub extension: String,
}

/// Increments whenever anything is copied. Reading it never triggers the paste-privacy prompt.
pub fn change_count() -> isize {
    NSPasteboard::generalPasteboard().changeCount()
}

/// Checks the advertised types only (no content read, so no privacy prompt).
pub fn has_image() -> bool {
    let pasteboard = NSPasteboard::generalPasteboard();
    let Some(types) = pasteboard.types() else { return false };
    let image_types = ["public.png", "public.tiff", "public.jpeg", "public.heic"];
    types.iter().any(|t| {
        let t = t.to_string();
        image_types.contains(&t.as_str()) || (t == "public.file-url" && file_image(&pasteboard).is_some())
    })
}

pub fn read_image() -> Option<ClipboardImage> {
    let pasteboard = NSPasteboard::generalPasteboard();
    let types: Vec<String> = pasteboard.types()?.iter().map(|t| t.to_string()).collect();
    let has = |t: &str| types.iter().any(|x| x == t);

    // A file copied in Finder also carries a TIFF of its icon, so check the file first.
    if has("public.file-url")
        && let Some(path) = file_image(&pasteboard)
    {
        let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
        let bytes = std::fs::read(&path).ok()?;
        if PORTABLE_EXTENSIONS.contains(&extension.as_str()) {
            return Some(ClipboardImage {
                bytes,
                extension: if extension == "jpeg" { "jpg".into() } else { extension },
            });
        }
        return to_png(&NSData::with_bytes(&bytes));
    }
    if has("public.png") {
        let data = pasteboard.dataForType(unsafe { NSPasteboardTypePNG })?;
        return Some(ClipboardImage { bytes: data.to_vec(), extension: "png".into() });
    }
    for t in ["public.tiff", "public.jpeg", "public.heic"] {
        if has(t) {
            let data = pasteboard.dataForType(&NSString::from_str(t))?;
            return to_png(&data);
        }
    }
    None
}

pub fn copy_text(text: &str) {
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    pasteboard.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString });
}

fn file_image(pasteboard: &NSPasteboard) -> Option<std::path::PathBuf> {
    let url = pasteboard.stringForType(unsafe { NSPasteboardTypeFileURL })?;
    let url = NSURL::URLWithString(&url)?;
    // Finder puts file *reference* URLs (file:///.file/id=…) on the pasteboard.
    let path = url.filePathURL()?.path()?.to_string();
    let path = std::path::PathBuf::from(path);
    let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
    let supported =
        PORTABLE_EXTENSIONS.contains(&extension.as_str()) || CONVERTIBLE_EXTENSIONS.contains(&extension.as_str());
    supported.then_some(path)
}

fn to_png(data: &NSData) -> Option<ClipboardImage> {
    let rep = NSBitmapImageRep::imageRepWithData(data)?;
    let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?;
    Some(ClipboardImage { bytes: png.to_vec(), extension: "png".into() })
}
