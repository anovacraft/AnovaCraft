//! Copy plot: one figure on the system clipboard in several formats at
//! once, so each program pastes the best one it reads.
//!
//! * macOS: PNG (`public.png`), PDF (`com.adobe.pdf`, which Keynote, Pages,
//!   Word and PowerPoint paste as a vector picture) and SVG
//!   (`public.svg-image`).
//! * Windows: a device-independent bitmap (the format most programs
//!   paste), PNG (the registered `PNG` format Office reads) and SVG
//!   (`image/svg+xml`). Windows programs paste vector pictures as EMF,
//!   which is not written, so the picture pastes as a bitmap there.
//! * Linux (X11): `image/png`, `application/pdf` and `image/svg+xml`.
//!
//! When the clipboard refuses the set, the PNG alone is tried before giving
//! up, and the outcome says which formats went on.

/// The platforms whose clipboard formats differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    MacOs,
    Windows,
    Linux,
}

impl Platform {
    /// The platform the app runs on.
    pub(crate) fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// One representation of the figure on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Entry {
    /// The platform's own bitmap, made from these PNG bytes.
    Bitmap(Vec<u8>),
    /// Bytes under a named clipboard format.
    Named(&'static str, Vec<u8>),
}

impl Entry {
    /// The clipboard format's name.
    pub(crate) fn format(&self) -> &'static str {
        match self {
            Self::Bitmap(_) => "bitmap",
            Self::Named(name, _) => name,
        }
    }
}

/// The figure as `platform`'s clipboard entries, best first.
pub(crate) fn payload(platform: Platform, png: Vec<u8>, pdf: Vec<u8>, svg: Vec<u8>) -> Vec<Entry> {
    match platform {
        Platform::MacOs => vec![
            Entry::Named("com.adobe.pdf", pdf),
            Entry::Named("public.png", png),
            Entry::Named("public.svg-image", svg),
        ],
        // The bitmap goes first: setting it empties the clipboard.
        Platform::Windows => {
            vec![Entry::Bitmap(png.clone()), Entry::Named("PNG", png), Entry::Named("image/svg+xml", svg)]
        }
        Platform::Linux => vec![
            Entry::Named("image/png", png),
            Entry::Named("application/pdf", pdf),
            Entry::Named("image/svg+xml", svg),
        ],
    }
}

/// The PNG entries of `entries` alone (the fallback when the full set fails).
fn png_only(entries: &[Entry]) -> Vec<Entry> {
    entries
        .iter()
        .filter(|e| matches!(e, Entry::Bitmap(_)) || matches!(e.format(), "public.png" | "PNG" | "image/png"))
        .cloned()
        .collect()
}

/// Put `entries` on the system clipboard. Returns what to tell the user.
///
/// # Errors
/// The clipboard cannot be opened or refuses even the PNG.
pub(crate) fn put(entries: Vec<Entry>) -> Result<String, String> {
    #[cfg(test)]
    {
        let formats = entries.iter().map(Entry::format).collect::<Vec<_>>();
        test_clipboard::record(&entries);
        Ok(format!("Copied the plot as {}", describe(&formats)))
    }
    #[cfg(not(test))]
    {
        let formats: Vec<&str> = entries.iter().map(Entry::format).collect();
        match system::set(&entries) {
            Ok(()) => Ok(format!("Copied the plot as {}", describe(&formats))),
            Err(first) => {
                let fallback = png_only(&entries);
                system::set(&fallback)
                    .map(|()| "Copied the plot as a picture (the clipboard did not take PDF or SVG)".to_owned())
                    .map_err(|_| format!("Could not copy the plot: {first}"))
            }
        }
    }
}

/// "PDF, PNG and SVG" for the formats put on.
fn describe(formats: &[&str]) -> String {
    let names: Vec<&str> = formats
        .iter()
        .map(|f| match *f {
            "com.adobe.pdf" | "application/pdf" => "PDF",
            "public.svg-image" | "image/svg+xml" => "SVG",
            "bitmap" => "a bitmap",
            _ => "PNG",
        })
        .collect();
    match names.split_last() {
        None => "nothing".to_owned(),
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(not(test))]
mod system {
    use super::Entry;
    use clipboard_rs::{Clipboard as _, ClipboardContent, ClipboardContext};
    use std::cell::RefCell;

    thread_local! {
        /// The clipboard, opened once: on X11 the app serves what it copied
        /// for as long as this lives.
        static CONTEXT: RefCell<Option<ClipboardContext>> = const { RefCell::new(None) };
    }

    fn content(entry: &Entry) -> Result<ClipboardContent, String> {
        match entry {
            #[cfg(target_os = "windows")]
            Entry::Bitmap(png) => {
                use clipboard_rs::common::RustImage as _;
                clipboard_rs::RustImageData::from_bytes(png).map(ClipboardContent::Image).map_err(|e| e.to_string())
            }
            #[cfg(not(target_os = "windows"))]
            Entry::Bitmap(png) => Ok(ClipboardContent::Other("image/png".to_owned(), png.clone())),
            Entry::Named(name, bytes) => Ok(ClipboardContent::Other((*name).to_owned(), bytes.clone())),
        }
    }

    pub(super) fn set(entries: &[Entry]) -> Result<(), String> {
        let contents = entries.iter().map(content).collect::<Result<Vec<_>, _>>()?;
        CONTEXT.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                *slot = Some(ClipboardContext::new().map_err(|e| e.to_string())?);
            }
            let ctx = slot.as_ref().expect("opened above");
            ctx.set(contents).map_err(|e| e.to_string())
        })
    }
}

/// What tests copied, in place of the system clipboard.
#[cfg(test)]
pub(crate) mod test_clipboard {
    use super::Entry;
    use std::cell::RefCell;

    thread_local! {
        static COPIED: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn record(entries: &[Entry]) {
        COPIED.with(|c| *c.borrow_mut() = entries.to_vec());
    }

    /// The entries copied last on this thread (and forget them).
    pub(crate) fn take() -> Vec<Entry> {
        COPIED.with(|c| std::mem::take(&mut *c.borrow_mut()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_platform_offers_its_own_formats() {
        let formats = |p| payload(p, vec![1], vec![2], vec![3]).iter().map(Entry::format).collect::<Vec<_>>();
        assert_eq!(formats(Platform::MacOs), ["com.adobe.pdf", "public.png", "public.svg-image"]);
        assert_eq!(formats(Platform::Windows), ["bitmap", "PNG", "image/svg+xml"]);
        assert_eq!(formats(Platform::Linux), ["image/png", "application/pdf", "image/svg+xml"]);
        // Each format carries its own file's bytes.
        let mac = payload(Platform::MacOs, vec![1], vec![2], vec![3]);
        assert_eq!(mac[0], Entry::Named("com.adobe.pdf", vec![2]));
        assert_eq!(mac[2], Entry::Named("public.svg-image", vec![3]));
        assert_eq!(payload(Platform::Windows, vec![1], vec![2], vec![3])[0], Entry::Bitmap(vec![1]));
    }

    #[test]
    fn the_fallback_keeps_only_the_picture() {
        for p in [Platform::MacOs, Platform::Windows, Platform::Linux] {
            let all = payload(p, vec![1], vec![2], vec![3]);
            let only = png_only(&all);
            assert!(!only.is_empty() && only.iter().all(|e| e.format() != "com.adobe.pdf"));
            assert!(only.iter().all(|e| !e.format().contains("svg") && !e.format().contains("pdf")));
        }
        assert_eq!(describe(&["com.adobe.pdf", "public.png", "public.svg-image"]), "PDF, PNG and SVG");
        assert_eq!(describe(&["bitmap", "PNG", "image/svg+xml"]), "a bitmap, PNG and SVG");
    }
}
