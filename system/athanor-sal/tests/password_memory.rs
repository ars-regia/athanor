//! A password field's text leaves no copy in memory Sal frees (doc_accessibility.md, AX4).
//! This test binary's allocator hands out zeroed blocks and searches every block it takes back
//! for the start of the secret, while the search is armed.

// A test-only allocator is the only way to see that no freed block holds a copy of the secret;
// it only delegates to the system's allocator and reads the blocks it is handed.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use athanor_sal::{Key, Theme, Ui};

/// The start of every secret below: a buffer that grows frees its smaller ones holding a
/// prefix of the text, so the start is what is searched for.
const START: &[u8] = b"Qz8%k";

static ARMED: AtomicBool = AtomicBool::new(false);
static FOUND: AtomicUsize = AtomicUsize::new(0);

/// The system's allocator, with each block zeroed when handed out, so every byte of it is
/// initialised when it is searched, and searched when it is taken back.
struct Searching;

// SAFETY: every block comes from the system allocator and goes back to it with the same layout;
// `dealloc` only reads the block it is given, which is still allocated, `layout.size()` bytes
// long and initialised, since `alloc` zeroed it and the default `realloc` goes through both.
unsafe impl GlobalAlloc for Searching {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract for `alloc` is that of `alloc_zeroed`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ARMED.load(Ordering::SeqCst) {
            // SAFETY: `ptr` is a live, initialised block of `layout.size()` bytes until it is
            // given back below.
            let block = unsafe { std::slice::from_raw_parts(ptr, layout.size()) };
            if block.windows(START.len()).any(|bytes| bytes == START) {
                FOUND.fetch_add(1, Ordering::SeqCst);
            }
        }
        // SAFETY: the caller's contract is passed on.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Searching = Searching;

/// Typed, committed by an input method and set by an assistive technology, each longer than
/// the 8 and 16 bytes a growing buffer starts at, then cleared and the field dropped: no block
/// freed meanwhile holds the start of any of them.
#[test]
fn a_password_typed_committed_or_set_leaves_no_copy_in_freed_memory() {
    let mut ui = Ui::new("Wi-Fi", Theme::default(), false);
    let field = ui.password(Ui::ROOT, "Password");
    ui.layout();
    ui.set_focus(Some(field));
    ARMED.store(true, Ordering::SeqCst);
    ui.key(Key::Text("Qz8%k typed in one key".into()));
    ui.commit("Qz8%k committed by an input method");
    ui.set_field_text(field, "Qz8%k set by an assistive technology\u{7}");
    let kept = ui.password_text(field) == Some("Qz8%k set by an assistive technology");
    ui.set_field_text(field, "");
    drop(ui);
    ARMED.store(false, Ordering::SeqCst);
    assert!(kept, "the field kept the text set");
    assert_eq!(
        FOUND.load(Ordering::SeqCst),
        0,
        "freed blocks held the secret"
    );
}
