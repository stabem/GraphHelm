//! #41: an allocation failure aborts the process past the panic hook, so the lifecycle record
//! would stay `running` and the next start could only say "vanished". This allocator forwards
//! every call to `System`; when `System` returns null it writes one note beside the record,
//! through a file opened at start, then returns null so std aborts exactly as before. The
//! failure path never allocates, panics or unwinds: the note is formatted into a stack buffer.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fs::File;
use std::io::Write;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) struct Guard;

static NOTE: OnceLock<File> = OnceLock::new();
static WRITTEN: AtomicBool = AtomicBool::new(false);

/// Debug builds only: `GRAPHHELM_DEBUG_FAIL_ALLOC_BYTES=<n>` makes any request of at least
/// `n` bytes fail, so an observer can stage the abort without exhausting the machine.
#[cfg(debug_assertions)]
static FAIL_AT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Hands the guard the opened, empty note file. Only the first call in a process takes effect.
pub(super) fn arm(note: File) {
    #[cfg(debug_assertions)]
    if let Some(bytes) = std::env::var("GRAPHHELM_DEBUG_FAIL_ALLOC_BYTES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
    {
        FAIL_AT.store(bytes, Ordering::Relaxed);
    }
    let _ = NOTE.set(note);
}

/// Formats `{"size":<size>,"at":<at>}\n` without allocating and writes it in one call.
pub(super) fn write_note(mut note: &File, size: usize, at: u64) {
    let mut buffer = [0_u8; 64];
    let mut length = 0;
    for part in [
        Part::Text(b"{\"size\":"),
        Part::Number(size as u64),
        Part::Text(b",\"at\":"),
        Part::Number(at),
        Part::Text(b"}\n"),
    ] {
        match part {
            Part::Text(text) => {
                buffer[length..length + text.len()].copy_from_slice(text);
                length += text.len();
            }
            Part::Number(mut value) => {
                let mut digits = [0_u8; 20];
                let mut count = 0;
                loop {
                    digits[count] = b'0' + (value % 10) as u8;
                    count += 1;
                    value /= 10;
                    if value == 0 {
                        break;
                    }
                }
                for digit in digits[..count].iter().rev() {
                    buffer[length] = *digit;
                    length += 1;
                }
            }
        }
    }
    let _ = note.write_all(&buffer[..length]);
    let _ = note.sync_data();
}

enum Part {
    Text(&'static [u8]),
    Number(u64),
}

fn failed(size: usize) {
    if WRITTEN.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Some(note) = NOTE.get() {
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        write_note(note, size, at);
    }
}

#[cfg(debug_assertions)]
fn staged(size: usize) -> bool {
    let at = FAIL_AT.load(Ordering::Relaxed);
    at != 0 && size >= at
}

#[cfg(not(debug_assertions))]
fn staged(_size: usize) -> bool {
    false
}

// SAFETY: every method forwards to `System` with the caller's arguments unchanged; the only
// addition is the note written when the result is already null.
unsafe impl GlobalAlloc for Guard {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = if staged(layout.size()) {
            std::ptr::null_mut()
        } else {
            // SAFETY: forwarded unchanged from the caller's contract.
            unsafe { System.alloc(layout) }
        };
        if pointer.is_null() {
            failed(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = if staged(layout.size()) {
            std::ptr::null_mut()
        } else {
            // SAFETY: forwarded unchanged from the caller's contract.
            unsafe { System.alloc_zeroed(layout) }
        };
        if pointer.is_null() {
            failed(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: forwarded unchanged from the caller's contract.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = if staged(size) {
            std::ptr::null_mut()
        } else {
            // SAFETY: forwarded unchanged from the caller's contract.
            unsafe { System.realloc(pointer, layout, size) }
        };
        if result.is_null() {
            failed(size);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    // The note is what the next start parses; a malformed number would read as no note.
    // Cost: one temporary file; drives the writer, not a real allocation failure.
    #[test]
    fn note_is_the_json_the_next_start_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note");
        let file = std::fs::File::create(&path).unwrap();
        super::write_note(&file, usize::MAX, 0);
        drop(file);
        let note: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(note["size"], usize::MAX as u64);
        assert_eq!(note["at"], 0);
    }
}
