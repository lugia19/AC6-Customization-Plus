//! The player's thruster flames take the color of the THRUSTER row in the Paint screen (Armored Core VI 1.09.1). The
//! row and its color come from NativePaint (native_paint.dll, row 0: saved with the AC, white until picked).
//! Also brings back the DEVICE row's brightness setting (device.rs) and adds a HUD row (hud.rs, NativePaint's row 1).
//!
//! Hooks:
//! - 0x142b56480 FXR external value read: serves the color to the flames, in thrusters.rs.
//! - 0x140937cc0 / 0x14092bcd0 the two flame-carrying slot updates: whether a flame is the player's, in
//!   player_flames.rs.
//! - Four Paint screen functions, in device.rs.
//! - 0x141609040 menu movie lookup and 0x1415f40b0 the Scaleform file opener: the HUD's look, in hud.rs.
//!
//! Logs to Game\debug-logs\paint_menu.log.

mod device;
mod hud;
mod hud_movie;
mod native_paint_client;
mod player_flames;
mod thrusters;

use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use retour::RawDetour;

static LOG: OnceLock<Mutex<File>> = OnceLock::new();

pub(crate) fn log(msg: &str) {
    if let Some(mut f) = LOG.get().and_then(|l| l.lock().ok()) {
        let _ = writeln!(f, "{msg}");
    }
}

unsafe extern "system" {
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
    let [thruster_slot_update, multi_sfx_slot_update] = player_flames::hooks();
    let [is_normal_slot, make_panel, build_panel, material_text] = device::hooks();
    let [movie_lookup, open_file] = hud::hooks();
    let hooks: [Hook; 9] = [
        thrusters::hook(),
        thruster_slot_update,
        multi_sfx_slot_update,
        is_normal_slot,
        make_panel,
        build_panel,
        material_text,
        movie_lookup,
        open_file,
    ];
    for &(addr, bytes, _, _) in &hooks {
        let actual = unsafe { std::slice::from_raw_parts(addr as *const u8, bytes.len()) };
        // A jmp means another DLL hooked it first; the detours chain fine.
        if actual != bytes && actual[0] != 0xe9 {
            return Err(format!("prologue at {addr:#x} doesn't match (not 1.09.1?)"));
        }
    }
    let mut detours = Vec::new();
    for (target, _, detour, trampoline) in hooks {
        let d = unsafe { RawDetour::new(target as *const (), detour) }.map_err(|e| e.to_string())?;
        trampoline.store(d.trampoline() as *const () as usize, Ordering::Relaxed);
        detours.push(d);
    }
    for d in &detours {
        unsafe { d.enable() }.map_err(|e| e.to_string())?;
    }
    // Hooks stay installed for the life of the process.
    std::mem::forget(detours);
    Ok(())
}

fn init() {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("debug-logs")))
        .unwrap_or_else(|| "debug-logs".into());
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(f) = File::create(dir.join("paint_menu.log")) {
        let _ = LOG.set(Mutex::new(f));
    }
    match install() {
        Ok(()) => {
            log("hooks installed");
            thrusters::start();
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
