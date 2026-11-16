//! Client for native_paint.dll: copy into your mod, `mod native_paint_client;`.
//! Resolves exports on first call (load order doesn't matter); no-op without the DLL.
//! A row is one color for the whole AC, not per part.
//!
//! ```ignore
//! // Until it returns true (the DLL may load after yours):
//! native_paint_client::claim_row(ROW_THRUSTER, "THRUSTER", 0xFFFFFF);
//! // Then, whenever you need it:
//! if let Some(rgb) = native_paint_client::color(ROW_THRUSTER) { ... }
//! ```

#![allow(dead_code)]

use std::sync::OnceLock;

// Rows (top to bottom), by client.
pub const ROW_THRUSTER: u32 = 0;
pub const ROW_HUD: u32 = 1;

type ClaimRowFn = unsafe extern "C" fn(u32, *const u8, usize, u32) -> bool;
type ColorFn = unsafe extern "C" fn(u32, *mut u32) -> bool;
type VersionFn = extern "C" fn() -> u32;

struct Api {
    claim_row: ClaimRowFn,
    color: ColorFn,
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
    let claim_row: ClaimRowFn = unsafe { std::mem::transmute(find(b"native_paint_claim_row\0")?) };
    let color: ColorFn = unsafe { std::mem::transmute(find(b"native_paint_color\0")?) };
    Some(API.get_or_init(|| Api { claim_row, color }))
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
