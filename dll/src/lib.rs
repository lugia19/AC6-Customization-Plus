//! More to set in the Paint screen (Armored Core VI 1.09.1): rows below WEATHERING, for the whole AC, and the DEVICE
//! row's hidden brightness setting.
//! - rows.rs: the extra rows themselves (a color, optionally a choice and more colors; saved with the AC).
//! - panel.rs: the rows' panels, and DEVICE's (its brightness); ui.rs: the game's menu plumbing they use.
//! - thrusters.rs: the GENERATOR row on the player's thruster flames.
//! - hud.rs, hud_movie.rs: the HUD row (color, opacity, warning color) on the in-mission HUD.
//! - weapons.rs: WEAPON VFX, the player's melee weapons' colors (a settings file next to the DLL for now).
//!
//! Hooks:
//! - 4 Paint list functions, in rows.rs; the panels' setup and the material text (DEVICE's labels), in panel.rs.
//! - 0x142b56480 FXR external value read and 0x140829de0 a character's generator color type: the player's flame type
//!   and color, in thrusters.rs.
//! - 0x141609040 menu movie lookup and 0x1415f40b0 the Scaleform file opener: the HUD's look, in hud.rs.
//! - 0x1410d8460 AC parts loader and 0x1415d0080 part name builder: the player's blade parts' copies, in weapons.rs.
//! The game's Arxan protection restores code it guards to its original bytes at irregular times, silently removing
//! hooks: the mod's me3 profile turns it off (disable_arxan).
//!
//! Logs to Game\debug-logs\customization_plus.log.

mod hud;
mod hud_movie;
mod paint_data;
mod panel;
mod rows;
mod thrusters;
mod ui;
mod weapons;

use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use retour::RawDetour;

static LOG: OnceLock<Mutex<File>> = OnceLock::new();
static MODULE: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn log(msg: &str) {
    if let Some(mut f) = LOG.get().and_then(|l| l.lock().ok()) {
        let _ = writeln!(f, "{msg}");
    }
}

unsafe extern "system" {
    fn GetCurrentProcess() -> isize;
    fn ReadProcessMemory(process: isize, address: usize, buffer: *mut u8, size: usize, read: *mut usize) -> i32;
    fn GetModuleFileNameW(module: usize, name: *mut u16, size: u32) -> u32;
}

const WORLD_CHR_MAN_ADDR: usize = 0x144e0f598;
const PLAYER_INS_OFFSET: usize = 0xA5A0;

/// The player's ChrIns (none while loading).
pub(crate) fn player() -> Option<usize> {
    let world_chr_man = read::<usize>(WORLD_CHR_MAN_ADDR).filter(|&p| p != 0)?;
    read::<usize>(world_chr_man + PLAYER_INS_OFFSET).filter(|&p| p != 0)
}

/// The folder this DLL is in.
pub(crate) fn module_dir() -> std::path::PathBuf {
    let mut name = [0u16; 1024];
    let length = unsafe { GetModuleFileNameW(MODULE.load(Ordering::Relaxed), name.as_mut_ptr(), name.len() as u32) };
    let path = std::path::PathBuf::from(String::from_utf16_lossy(&name[..length as usize]));
    path.parent().map(|dir| dir.to_path_buf()).unwrap_or_default()
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
    let mut hooks: Vec<Hook> = thrusters::hooks().into();
    hooks.extend(rows::hooks());
    hooks.extend(hud::hooks());
    hooks.extend(panel::hooks());
    hooks.extend(weapons::hooks());
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
    if let Ok(f) = File::create(dir.join("customization_plus.log")) {
        let _ = LOG.set(Mutex::new(f));
    }
    // Before the hooks go live: a Paint list built after that has them.
    thrusters::claim_row();
    hud::claim_row();
    match install() {
        Ok(()) => {
            log("hooks installed");
            thrusters::start();
            weapons::start();
        }
        Err(e) => log(&format!("not hooked: {e}")),
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllMain(module: *mut u8, reason: u32, _reserved: *mut u8) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    if reason == DLL_PROCESS_ATTACH {
        MODULE.store(module as usize, Ordering::Relaxed);
        init();
    }
    1
}
