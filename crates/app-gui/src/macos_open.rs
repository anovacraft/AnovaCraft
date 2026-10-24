//! macOS: receive files opened from Finder (double-click, Open With, or
//! dropping them on the Dock icon).
//!
//! macOS asks the application to open documents with an "open documents"
//! Apple Event (`'aevt'`/`'odoc'`). The window library does not handle it, so
//! AppKit's default handler answered "cannot open files of this type". This
//! module installs its own handler with the Apple Event Manager C API, at the
//! moment Apple documents for doing so — when the application posts
//! `NSApplicationWillFinishLaunchingNotification`, after AppKit has installed
//! its default handlers and before the launch event is dispatched — observed
//! through the CoreFoundation notification centre (the same centre as
//! `NSNotificationCenter`'s default centre). The handler only records the
//! files' paths; the app opens them on its next frame through the ordinary
//! open path.
//!
//! # Safety
//! This is the only module in the workspace that uses `unsafe`: plain C calls
//! into CoreFoundation and the Apple Event Manager (part of CoreServices),
//! with the types and constants of their public headers. Every pointer passed
//! in is either a valid local, a `'static` C string, or one handed to us by
//! the system for the duration of the callback; every descriptor obtained is
//! disposed of; nothing is retained across calls except plain Rust data
//! behind a mutex.
#![allow(unsafe_code)]

use std::ffi::{c_char, c_long, c_void};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use eframe::egui;

type OSErr = i16;
type FourCharCode = u32;

/// An Apple Event descriptor (`AEDesc` in `AEDataModel.h`).
#[repr(C)]
struct AEDesc {
    descriptor_type: FourCharCode,
    data_handle: *mut c_void,
}

impl AEDesc {
    fn null() -> Self {
        Self { descriptor_type: TYPE_NULL, data_handle: std::ptr::null_mut() }
    }
}

type AEEventHandlerProc = extern "C" fn(event: *const AEDesc, reply: *mut AEDesc, refcon: *mut c_void) -> OSErr;
type CFNotificationCallback = extern "C" fn(
    center: *mut c_void,
    observer: *mut c_void,
    name: *const c_void,
    object: *const c_void,
    user_info: *const c_void,
);

const fn four_cc(code: &[u8; 4]) -> FourCharCode {
    ((code[0] as u32) << 24) | ((code[1] as u32) << 16) | ((code[2] as u32) << 8) | code[3] as u32
}

const K_CORE_EVENT_CLASS: FourCharCode = four_cc(b"aevt");
const K_AE_OPEN_DOCUMENTS: FourCharCode = four_cc(b"odoc");
const KEY_DIRECT_OBJECT: FourCharCode = four_cc(b"----");
const TYPE_AE_LIST: FourCharCode = four_cc(b"list");
const TYPE_FILE_URL: FourCharCode = four_cc(b"furl");
const TYPE_NULL: FourCharCode = four_cc(b"null");
const NO_ERR: OSErr = 0;
const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
/// `CFNotificationSuspensionBehaviorDeliverImmediately`.
const DELIVER_IMMEDIATELY: isize = 4;

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn AEInstallEventHandler(
        event_class: FourCharCode,
        event_id: FourCharCode,
        handler: AEEventHandlerProc,
        refcon: *mut c_void,
        is_sys_handler: u8,
    ) -> OSErr;
    fn AEGetParamDesc(
        event: *const AEDesc,
        keyword: FourCharCode,
        desired_type: FourCharCode,
        result: *mut AEDesc,
    ) -> OSErr;
    fn AECountItems(list: *const AEDesc, count: *mut c_long) -> OSErr;
    fn AEGetNthPtr(
        list: *const AEDesc,
        index: c_long,
        desired_type: FourCharCode,
        keyword: *mut FourCharCode,
        type_code: *mut FourCharCode,
        data: *mut c_void,
        max_size: isize,
        actual_size: *mut isize,
    ) -> OSErr;
    fn AEDisposeDesc(desc: *mut AEDesc) -> OSErr;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFNotificationCenterGetLocalCenter() -> *mut c_void;
    fn CFNotificationCenterAddObserver(
        center: *mut c_void,
        observer: *const c_void,
        callback: CFNotificationCallback,
        name: *const c_void,
        object: *const c_void,
        suspension_behavior: isize,
    );
    fn CFStringCreateWithCString(allocator: *const c_void, c_str: *const c_char, encoding: u32) -> *const c_void;
}

/// Files received but not yet opened by the app.
static PENDING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
/// The UI context, to wake the app when files arrive while it is idle.
static CONTEXT: OnceLock<egui::Context> = OnceLock::new();
/// A distinct address identifying our notification observer.
static OBSERVER: u8 = 0;

/// Arrange to receive open-document requests. Call once, before the event
/// loop starts.
pub fn install() {
    // SAFETY: the name is a 'static NUL-terminated string; the returned
    // CFString is intentionally kept for the life of the process (the centre
    // may retain or compare it). The observer pointer is a 'static address
    // used only as an identity, and the callback has the C signature the
    // centre expects.
    unsafe {
        let name = CFStringCreateWithCString(
            std::ptr::null(),
            c"NSApplicationWillFinishLaunchingNotification".as_ptr(),
            K_CF_STRING_ENCODING_UTF8,
        );
        if name.is_null() {
            return;
        }
        CFNotificationCenterAddObserver(
            CFNotificationCenterGetLocalCenter(),
            std::ptr::from_ref(&OBSERVER).cast(),
            will_finish_launching,
            name,
            std::ptr::null(),
            DELIVER_IMMEDIATELY,
        );
    }
}

/// Remember the UI context so arriving files wake the app, and make sure the
/// handler is installed (a no-op repeat if launch already installed it).
pub fn set_context(ctx: &egui::Context) {
    let _ = CONTEXT.set(ctx.clone());
    install_handler();
}

fn install_handler() {
    // SAFETY: installs a C function with the AEEventHandlerProcPtr signature
    // for this application's (not the system's) handler table.
    unsafe {
        AEInstallEventHandler(K_CORE_EVENT_CLASS, K_AE_OPEN_DOCUMENTS, open_documents, std::ptr::null_mut(), 0);
    }
}

/// Files received since the last call.
pub fn take_pending() -> Vec<PathBuf> {
    PENDING.lock().map(|mut p| std::mem::take(&mut *p)).unwrap_or_default()
}

extern "C" fn will_finish_launching(
    _center: *mut c_void,
    _observer: *mut c_void,
    _name: *const c_void,
    _object: *const c_void,
    _user_info: *const c_void,
) {
    install_handler();
}

extern "C" fn open_documents(event: *const AEDesc, _reply: *mut AEDesc, _refcon: *mut c_void) -> OSErr {
    let mut paths = Vec::new();
    let mut list = AEDesc::null();
    // SAFETY: `event` is the event the Apple Event Manager is dispatching,
    // valid for this call. `list` is a local descriptor filled by
    // AEGetParamDesc and disposed of below; each item is copied into a local
    // buffer whose length is passed, and its actual size is checked before use.
    unsafe {
        if AEGetParamDesc(event, KEY_DIRECT_OBJECT, TYPE_AE_LIST, &mut list) != NO_ERR {
            return NO_ERR;
        }
        let mut count: c_long = 0;
        if AECountItems(&list, &mut count) == NO_ERR {
            for index in 1..=count {
                let mut buffer = vec![0u8; 4096];
                let (mut keyword, mut type_code, mut size) = (0, 0, 0_isize);
                let status = AEGetNthPtr(
                    &list,
                    index,
                    TYPE_FILE_URL,
                    &mut keyword,
                    &mut type_code,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as isize,
                    &mut size,
                );
                if status == NO_ERR && size > 0 && (size as usize) <= buffer.len() {
                    buffer.truncate(size as usize);
                    if let Some(path) = crate::file_io::path_from_file_url(&String::from_utf8_lossy(&buffer)) {
                        paths.push(path);
                    }
                }
            }
        }
        AEDisposeDesc(&mut list);
    }
    if !paths.is_empty() {
        if let Ok(mut pending) = PENDING.lock() {
            pending.extend(paths);
        }
        if let Some(ctx) = CONTEXT.get() {
            ctx.request_repaint();
        }
    }
    NO_ERR
}
