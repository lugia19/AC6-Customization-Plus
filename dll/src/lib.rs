//! More to set in the Paint screen (Armored Core VI 1.09.1): rows below WEATHERING, for the whole AC, and the DEVICE
//! row's hidden brightness setting.
//! - rows.rs: the extra rows themselves (a color, optionally a choice and a second color; saved with the AC).
//! - panel.rs: the panels of the rows with choices and of DEVICE; ui.rs: the game's menu plumbing they use.
//! - thrusters.rs, player_flames.rs: the THRUSTER row on the player's thruster flames.
//! - hud.rs, hud_movie.rs: the HUD row (color, opacity, warning color) on the in-mission HUD.
//! - device.rs: DEVICE's brightness.
//!
//! Hooks:
//! - 9 Paint list functions, in rows.rs; the panels' setup, in panel.rs.
//! - 0x142b56480 FXR external value read: serves the color to the flames, in thrusters.rs.
//! - 0x141609040 menu movie lookup and 0x1415f40b0 the Scaleform file opener: the HUD's look, in hud.rs.
//! Plus the two flame-carrying slot updates, replaced in their classes' vtables: whether a flame is the player's, in
//! player_flames.rs.
//!
//! Logs to Game\debug-logs\customization_plus.log.

mod device;
mod hud;
mod hud_movie;
mod memlog; // TEMPORARY
mod paint_data;
mod player_flames;
mod panel;
mod rows;
mod thrusters;
mod ui;

use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use retour::RawDetour;

static LOG: OnceLock<Mutex<File>> = OnceLock::new();
// Each hook's address and its first bytes once hooked (the jump to our detour), for keep_hooks().
static HOOKED: OnceLock<Vec<(usize, [u8; 6])>> = OnceLock::new();

pub(crate) fn log(msg: &str) {
    if let Some(mut f) = LOG.get().and_then(|l| l.lock().ok()) {
        let _ = writeln!(f, "{msg}");
    }
}

unsafe extern "system" {
    fn VirtualProtect(address: *mut u8, size: usize, protect: u32, old_protect: *mut u32) -> i32;
    fn GetCurrentProcess() -> isize;
    fn ReadProcessMemory(process: isize, address: usize, buffer: *mut u8, size: usize, read: *mut usize) -> i32;
}

/// Reads memory that may not be mapped (pointer chains while loading, effects being freed) without faulting.
pub(crate) fn read<T: Copy + Default>(addr: usize) -> Option<T> {
    let mut v = T::default();
    let mut n = 0;
    let ok = unsafe { ReadProcessMemory(GetCurrentProcess(), addr, &mut v as *mut T as *mut u8, size_of::<T>(), &mut n) };
    (ok != 0 && n == size_of::<T>()).then_some(v)
}

/// A hooked function's original (its trampoline), typed as `F` (an `unsafe extern "system" fn`).
pub(crate) unsafe fn orig<F: Copy>(trampoline: &AtomicUsize) -> F {
    unsafe { std::mem::transmute_copy(&trampoline.load(Ordering::Relaxed)) }
}

/// A hook: target address, its first bytes (checked before hooking), detour, trampoline slot.
pub(crate) type Hook = (usize, [u8; 6], *const (), &'static AtomicUsize);

fn install() -> Result<(), String> {
    let mut hooks: Vec<Hook> = vec![thrusters::hook()];
    hooks.extend(rows::hooks());
    hooks.extend(hud::hooks());
    hooks.extend(panel::hooks());
    for &(addr, bytes, _, _) in &hooks {
        let actual = unsafe { std::slice::from_raw_parts(addr as *const u8, bytes.len()) };
        // A jmp means another DLL hooked it first; the detours chain fine.
        if actual != bytes && actual[0] != 0xe9 {
            return Err(format!("prologue at {addr:#x} doesn't match (not 1.09.1?)"));
        }
    }
    let targets: Vec<usize> = hooks.iter().map(|hook| hook.0).collect();
    let mut detours = Vec::new();
    for (target, _, detour, trampoline) in hooks {
        let d = unsafe { RawDetour::new(target as *const (), detour) }.map_err(|e| e.to_string())?;
        trampoline.store(d.trampoline() as *const () as usize, Ordering::Relaxed);
        detours.push(d);
    }
    for d in &detours {
        unsafe { d.enable() }.map_err(|e| e.to_string())?;
    }
    let _ = HOOKED.set(targets.iter().map(|&addr| (addr, unsafe { *(addr as *const [u8; 6]) })).collect());
    // Hooks stay installed for the life of the process.
    std::mem::forget(detours);
    player_flames::install()
}

/// Writes `value` at `address` in the game's image (code or a vtable), lifting the page's protection for it.
pub(crate) unsafe fn write_protected<T: Copy>(address: usize, value: T) -> bool {
    const PAGE_EXECUTE_READWRITE: u32 = 0x40;
    let mut old = 0;
    if unsafe { VirtualProtect(address as *mut u8, size_of::<T>(), PAGE_EXECUTE_READWRITE, &mut old) } == 0 {
        return false;
    }
    unsafe {
        (address as *mut T).write_unaligned(value);
        VirtualProtect(address as *mut u8, size_of::<T>(), old, &mut old);
    }
    true
}

/// Without me3's disable_arxan, the game's Arxan protection restores code it guards to its original bytes at
/// irregular times (seen on two functions, minutes to over an hour into a session), silently removing a hook. This
/// puts back, 5 times a second, any hook whose bytes changed.
fn keep_hooks() {
    std::thread::spawn(|| {
        let mut failed = std::collections::HashSet::new();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(200));
            for &(addr, hooked) in HOOKED.get().into_iter().flatten() {
                if unsafe { *(addr as *const [u8; 6]) } == hooked {
                    continue;
                }
                if unsafe { write_protected(addr, hooked) } {
                    log(&format!("the hook at {addr:#x} was removed: put back"));
                } else if failed.insert(addr) {
                    log(&format!("the hook at {addr:#x} was removed and can't be put back"));
                }
            }
        }
    });
}

fn init() {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("debug-logs")))
        .unwrap_or_else(|| "debug-logs".into());
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(f) = File::create(dir.join("customization_plus.log")) {
        let _ = LOG.set(Mutex::new(f));
    }
    match install() {
        Ok(()) => {
            log("hooks installed");
            keep_hooks();
            thrusters::start();
            hud::claim_rows();
            memlog::start(); // TEMPORARY
        }
        Err(e) => log(&format!("not hooked: {e}")),
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllMain(_module: *mut u8, reason: u32, _reserved: *mut u8) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    if reason == DLL_PROCESS_ATTACH {
        init();
    }
    1
}
