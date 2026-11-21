//! Our own effect and material archives, loaded next to the game's instead of replacing them (docs/extra-archives.md):
//! they hold only what the mod adds, so mods that ship the game's archives still work. Both are kept for the session.
//! - Materials: after the boot material load 0x14129dbf0 (AllMaterial), `Load<MtdbndFileCap>(CSFile, path, 0)`.
//!   Before any effect model is created, which binds its materials by name once.
//! - Effects: after InGameStayStep's load 0x141260380 (SfxBnd_CommonEffects, once per process), the effect manager's
//!   `load(manager, path, load group)` with the step's load group, so its load wait covers ours too.
//! The paths are build.py's OWN_EFFECTS / OWN_MATERIALS.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::game::{self, game_fn, utf16};
use crate::{Hook, log, orig};

static EFFECTS_PATH: [u16; 40] = utf16(b"sfxbnd:/SfxBnd_CustomizationPlus.ffxbnd\0");
static MATERIALS_PATH: [u16; 38] = utf16(b"material:/CustomizationPlus.matbinbnd\0");

// `handle load(effect manager, const wchar_t* path, load group)`; the manager; the step's load group.
const EFFECT_LOAD_ADDR: usize = 0x141623c10;
const EFFECT_MANAGER_ADDR: usize = 0x144e29820;
const STEP_LOAD_GROUP_OFFSET: usize = 0xb8;
// `handle Load<MtdbndFileCap>(CSFile, const wchar_t* path, 0)`; CSFile.
const MATERIAL_LOAD_ADDR: usize = 0x1403d2870;
const CS_FILE_ADDR: usize = 0x144e00c50;

type StepFn = unsafe extern "system" fn(usize);
type LoadFn = unsafe extern "system" fn(usize, *const u16, usize) -> usize;

static ORIG_STAY_LOAD: AtomicUsize = AtomicUsize::new(0);
static ORIG_MATERIALS_STEP: AtomicUsize = AtomicUsize::new(0);
static EFFECTS: AtomicUsize = AtomicUsize::new(0);
static MATERIALS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn hooks() -> [Hook; 2] {
    [
        // mov [rsp+8],rbx / push rdi
        (0x141260380, [0x48, 0x89, 0x5c, 0x24, 0x08, 0x57], stay_load_impl as *const (), &ORIG_STAY_LOAD),
        // push rbx / sub rsp,20
        (0x14129dbf0, [0x40, 0x53, 0x48, 0x83, 0xec, 0x20], materials_step_impl as *const (), &ORIG_MATERIALS_STEP),
    ]
}

/// Loads an archive once (`held` keeps its handle; the debug reloads run these steps again).
fn load_once(held: &AtomicUsize, what: &str, load: impl FnOnce() -> Option<usize>) {
    if held.load(Ordering::Relaxed) != 0 {
        return;
    }
    match load().filter(|&handle| handle != 0) {
        Some(handle) => {
            held.store(handle, Ordering::Relaxed);
            log(&format!("archives: requested our {what}"));
        }
        None => log(&format!("archives: couldn't request our {what}")),
    }
}

unsafe extern "system" fn stay_load_impl(step: usize) {
    unsafe { orig::<StepFn>(&ORIG_STAY_LOAD)(step) };
    load_once(&EFFECTS, "effects", || {
        let manager = game::read::<usize>(EFFECT_MANAGER_ADDR).filter(|&m| m != 0)?;
        let load: LoadFn = unsafe { game_fn(EFFECT_LOAD_ADDR) };
        Some(unsafe { load(manager, EFFECTS_PATH.as_ptr(), step + STEP_LOAD_GROUP_OFFSET) })
    });
}

unsafe extern "system" fn materials_step_impl(step: usize) {
    unsafe { orig::<StepFn>(&ORIG_MATERIALS_STEP)(step) };
    load_once(&MATERIALS, "materials", || {
        let cs_file = game::read::<usize>(CS_FILE_ADDR).filter(|&f| f != 0)?;
        let load: LoadFn = unsafe { game_fn(MATERIAL_LOAD_ADDR) };
        Some(unsafe { load(cs_file, MATERIALS_PATH.as_ptr(), 0) })
    });
}
