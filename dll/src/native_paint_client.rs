//! Client for native_paint.dll: copy into your mod, `mod native_paint_client;`.
//! Resolves exports on first call (load order doesn't matter); no-op without the DLL.
//! A row is one color for the whole AC, not per part; optionally also a choice from a list.
//!
//! ```ignore
//! // Until it returns true (the DLL may load after yours):
//! native_paint_client::claim_row(ROW_THRUSTER, "THRUSTER", 0xFFFFFF);
//! // Then, whenever you need it:
//! if let Some(rgb) = native_paint_client::color(ROW_THRUSTER) { ... }
//!
//! // Optional, after the claim: a list of choices (needs material rows in your regulation; see set_choices).
//! native_paint_client::set_choices(ROW_THRUSTER, &["Normal", "Plasma", "Coral"], 0);
//! if let Some(index) = native_paint_client::choice(ROW_THRUSTER) { ... }
//! ```

#![allow(dead_code)]

use std::sync::OnceLock;

// Rows (top to bottom); all three are paint-menu's.
pub const ROW_THRUSTER: u32 = 0;
pub const ROW_HUD: u32 = 1;
pub const ROW_HUD_WARNING: u32 = 2;

/// How many choices a row can have.
pub const MAX_CHOICES: usize = 36;

/// The ID of the material a row's first choice needs in PlayerMaterialPresetParam; the others follow it.
pub const fn first_material(row: u32) -> u32 {
    100 + row * 100
}

// (row, text, text length, number) and (row, out)
type SetTextFn = unsafe extern "C" fn(u32, *const u8, usize, u32) -> bool;
type GetNumberFn = unsafe extern "C" fn(u32, *mut u32) -> bool;
type VersionFn = extern "C" fn() -> u32;

struct Api {
    claim_row: SetTextFn,
    color: GetNumberFn,
    set_choices: SetTextFn,
    choice: GetNumberFn,
}

unsafe extern "system" {
    fn GetModuleHandleA(name: *const u8) -> usize;
    fn GetProcAddress(module: usize, name: *const u8) -> usize;
}

const NEEDED_VERSION: u32 = 1;

static API: OnceLock<Api> = OnceLock::new();

// Cached once found; natives never unload.
fn api() -> Option<&'static Api> {
    if let Some(api) = API.get() {
        return Some(api);
    }
    let module = unsafe { GetModuleHandleA(b"native_paint.dll\0".as_ptr()) };
    if module == 0 {
        return None;
    }
    let find = |name: &[u8]| {
        let addr = unsafe { GetProcAddress(module, name.as_ptr()) };
        (addr != 0).then_some(addr)
    };
    let version: VersionFn = unsafe { std::mem::transmute(find(b"native_paint_api_version\0")?) };
    if version() < NEEDED_VERSION {
        return None;
    }
    let claim_row: SetTextFn = unsafe { std::mem::transmute(find(b"native_paint_claim_row\0")?) };
    let color: GetNumberFn = unsafe { std::mem::transmute(find(b"native_paint_color\0")?) };
    let set_choices: SetTextFn = unsafe { std::mem::transmute(find(b"native_paint_set_choices\0")?) };
    let choice: GetNumberFn = unsafe { std::mem::transmute(find(b"native_paint_choice\0")?) };
    Some(API.get_or_init(|| Api { claim_row, color, set_choices, choice }))
}

/// Shows a row in the Paint list, with `default_color` (0xRRGGBB) until the player picks one. False if the DLL is
/// missing (or not loaded yet) or refused it.
pub fn claim_row(row: u32, label: &str, default_color: u32) -> bool {
    api().is_some_and(|api| unsafe { (api.claim_row)(row, label.as_ptr(), label.len(), default_color) })
}

/// The row's color on the player's AC (0xRRGGBB): picked, or the default. None if the DLL is missing, the row isn't
/// claimed, or the player's data isn't loaded right now.
pub fn color(row: u32) -> Option<u32> {
    let mut rgb = 0;
    api().is_some_and(|api| unsafe { (api.color)(row, &mut rgb) }).then_some(rgb)
}

/// Gives a claimed row a list of choices (up to MAX_CHOICES one-line labels), listed in the row's Material menu.
/// They are materials: your regulation needs one row per label in PlayerMaterialPresetParam, with consecutive IDs
/// from first_material(row) (NativePaint's tools/add_choice_materials.py adds them). False if the DLL is missing or
/// refused them.
pub fn set_choices(row: u32, labels: &[&str], default_choice: u32) -> bool {
    let labels = labels.join("\n");
    api().is_some_and(|api| unsafe { (api.set_choices)(row, labels.as_ptr(), labels.len(), default_choice) })
}

/// The row's choice on the player's AC, as an index into its labels: picked, or the default. None if the DLL is
/// missing, the row has no choices, or the player's data isn't loaded right now.
pub fn choice(row: u32) -> Option<u32> {
    let mut index = 0;
    api().is_some_and(|api| unsafe { (api.choice)(row, &mut index) }).then_some(index)
}
