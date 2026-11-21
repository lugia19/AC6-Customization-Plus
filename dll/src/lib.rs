//! More to set in the Paint screen (Armored Core VI 1.09.1): rows below WEATHERING, for the whole AC, the DEVICE row's
//! hidden brightness setting, and melee weapon colors.
//! - Menus: rows.rs (the extra rows: a color, optionally a choice and more colors; saved with the AC), panel.rs (their
//!   panels, and DEVICE's brightness), ui.rs (the game's menu plumbing they use).
//! - Features: generator.rs (the GENERATOR row: flames, expansions), hud.rs / hud_movie.rs (the HUD row), weapons.rs
//!   (melee weapon colors: the WEAPON VFX row).
//! - Shared mechanisms: ext_values.rs (the colors served to the tinted effect copies), columns.rs (the player's own
//!   columns in the generator-dependent effect tables), game.rs (memory helpers, shared pointer chains).
//!
//! Hooks:
//! - 4 Paint list functions, in rows.rs; the panels' setup and the material text (DEVICE's labels), in panel.rs.
//! - 0x142b56480 FXR external value read, in ext_values.rs.
//! - 0x140829de0 a character's generator color type, 0x1415cdd90 / 0x1415ce050 the effect tables' lookups,
//!   0x140698380 the bullet effect spawn and 0x1408f0190 the muzzle flash spawn, in columns.rs.
//! - 0x141609040 menu movie lookup and 0x1415f40b0 the Scaleform file opener: the HUD's look, in hud.rs.
//! - 0x1410ff770 a part model's paint: the player's weapon models' color entries; 0x14102c4c0 the Paint preview's
//!   highlight: WEAPON VFX flashes the weapons; in weapons.rs.
//! - 0x140995870 / 0x140995780 hit effect dispatchers and 0x140b9f390 the hit effect list: the player's melee hits'
//!   tinted copies, in hits.rs.
//! The game's Arxan protection restores code it guards to its original bytes at irregular times, silently removing
//! hooks: the mod's me3 profile turns it off (disable_arxan).
//!
//! One thread applies the features' settings when they change (update_loop). Logs to
//! Game\debug-logs\customization_plus.log.

mod columns;
mod ext_values;
mod game;
mod generator;
mod hits;
mod hud;
mod hud_movie;
mod panel;
mod rows;
mod ui;
mod weapons;

use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use retour::RawDetour;

const UPDATE_INTERVAL: Duration = Duration::from_millis(200);

static LOG: OnceLock<Mutex<File>> = OnceLock::new();

pub(crate) fn log(msg: &str) {
    if let Some(mut f) = LOG.get().and_then(|l| l.lock().ok()) {
        let _ = writeln!(f, "{msg}");
    }
}

/// A hooked function's original (its trampoline), typed as `F` (an `unsafe extern "system" fn`).
pub(crate) unsafe fn orig<F: Copy>(trampoline: &AtomicUsize) -> F {
    unsafe { std::mem::transmute_copy(&trampoline.load(Ordering::Relaxed)) }
}

/// A hook: target address, its first bytes (checked before hooking), detour, trampoline slot.
pub(crate) type Hook = (usize, [u8; 6], *const (), &'static AtomicUsize);

fn install() -> Result<(), String> {
    let mut hooks: Vec<Hook> = ext_values::hooks().into();
    hooks.extend(columns::hooks());
    hooks.extend(rows::hooks());
    hooks.extend(hud::hooks());
    hooks.extend(panel::hooks());
    hooks.extend(weapons::hooks());
    hooks.extend(hits::hooks());
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

/// Applies the settings whenever they change: the rows' values, the weapon colors.
fn update_loop() {
    let mut generator = generator::Generator::new();
    let mut weapons = weapons::Weapons::default();
    loop {
        generator.update();
        weapons.update();
        std::thread::sleep(UPDATE_INTERVAL);
    }
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
    // Before the hooks go live: a Paint list built after that has them, and the external values are served.
    generator::claim_row();
    hud::claim_row();
    weapons::claim_row();
    ext_values::register(generator::ext_blocks().chain(weapons::ext_blocks()).collect());
    match install() {
        Ok(()) => {
            log("hooks installed");
            std::thread::spawn(update_loop);
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
