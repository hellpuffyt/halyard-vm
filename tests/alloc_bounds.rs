//! Untrusted snapshot/program bytes must not be able to make the host
//! allocate more than the input justifies.
//!
//! This test binary installs a counting allocator (test-only `unsafe`; the
//! library itself is `#![forbid(unsafe_code)]`) so the bound is checked
//! deterministically on every OS — macOS and Linux both overcommit lazily,
//! so "it didn't crash" would not be evidence.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use halyard::{Config, Program, Vm, asm};

struct Counting;
static LARGEST: AtomicUsize = AtomicUsize::new(0);
/// The counter is global, so the tests in this file take turns.
static SERIAL: Mutex<()> = Mutex::new(());

// SAFETY: forwards every call unchanged to the system allocator; only
// records the largest requested size.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LARGEST.fetch_max(l.size(), Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        LARGEST.fetch_max(n, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// Regression: `restore` called `Vec::with_capacity(n_events)` with an
/// untrusted 32-bit count, so changing 4 bytes of a snapshot made the host
/// request ~100 GB (an abort wherever overcommit refuses it).
#[test]
fn corrupt_snapshot_event_count_cannot_force_a_huge_allocation() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let p = asm::assemble("movi r1, 1\n halt").unwrap();
    let vm = Vm::new(&p, Config::default());
    let mut snap = vm.snapshot();
    let n = snap.len();
    // With no events, the event count is the last 4 bytes of the snapshot.
    snap[n - 4..].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut vm2 = Vm::new(&p, Config::default());
    LARGEST.store(0, Ordering::Relaxed);
    assert!(vm2.restore(&snap).is_err());
    let largest = LARGEST.load(Ordering::Relaxed);
    assert!(
        largest <= 16 * snap.len() + 4096,
        "restore of a {}-byte snapshot requested {largest} bytes",
        snap.len()
    );
}

#[test]
fn corrupt_program_header_cannot_force_a_huge_allocation() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let p = asm::assemble("movi r1, 1\n halt").unwrap();
    let mut bytes = p.to_bytes();
    bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    LARGEST.store(0, Ordering::Relaxed);
    assert!(Program::from_bytes(&bytes).is_err());
    assert!(LARGEST.load(Ordering::Relaxed) <= 4096);
}
