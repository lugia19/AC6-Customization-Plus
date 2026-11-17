//! Brings back the DEVICE row's brightness setting in the Paint screen.
//!
//! The game has it all, switched off at the row: DEVICE's material (materials 90-99, "Light Source Intensity", per
//! part like any material; the default is 92) drives how bright the AC's lights are, and the material menu lists
//! those ten for DEVICE with their own text. But the DEVICE row is built color-only, so nothing leads to that menu.
//!
//! Hooks:
//! - 0x14045de80 is-normal-slot (the row builder's only check): true for DEVICE, so its row is built like
//!   MAIN..OTHER: confirming it opens the Color / Material panel instead of the color picker.
//! - 0x140f43610 (list, out, row kind) makes that panel: a switch with a case per kind, 0-4 calling the panel builder
//!   with the slot as a constant, nothing for 5. For kind 5 the hook runs case 0 and
//! - 0x140f49590 panel builder (.., .., slot): the slot is swapped for DEVICE's meanwhile.
//! - 0x140e0cc30 (out, material) -> the panel's Material text ("Reflectiveness: x  Luster: y"). The material menu's
//!   text function 0x140f35c40 has a DEVICE branch ("Light Source Intensity" + the material row's float at +8) that
//!   the panel lacks: for DEVICE's materials the hook calls it.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::{Hook, orig};

const DEVICE_SLOT: u8 = 5;
const DEVICE_MATERIALS: std::ops::RangeInclusive<u32> = 90..=99;
// Material -> its param row.
const MATERIAL_ROW_ADDR: usize = 0x140e0cdd0;
// The material menu's text for a material: (info, out) -> out.
const MENU_MATERIAL_TEXT_ADDR: usize = 0x140f35c40;

static ORIG_IS_NORMAL_SLOT: AtomicUsize = AtomicUsize::new(0);
static ORIG_MAKE_PANEL: AtomicUsize = AtomicUsize::new(0);
static ORIG_BUILD_PANEL: AtomicUsize = AtomicUsize::new(0);
static ORIG_MATERIAL_TEXT: AtomicUsize = AtomicUsize::new(0);
// Set while the panel being made is DEVICE's (menu code runs on one thread).
static MAKING_DEVICE_PANEL: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn is_normal_slot_impl(slot: *const u8) -> bool {
    unsafe { *slot == DEVICE_SLOT || orig::<unsafe extern "system" fn(*const u8) -> bool>(&ORIG_IS_NORMAL_SLOT)(slot) }
}

type MakePanelFn = unsafe extern "system" fn(usize, usize, u32) -> usize;

unsafe extern "system" fn make_panel_impl(list: usize, out: usize, kind: u32) -> usize {
    if kind != DEVICE_SLOT as u32 {
        return unsafe { orig::<MakePanelFn>(&ORIG_MAKE_PANEL)(list, out, kind) };
    }
    MAKING_DEVICE_PANEL.store(true, Ordering::Relaxed);
    let result = unsafe { orig::<MakePanelFn>(&ORIG_MAKE_PANEL)(list, out, 0) };
    MAKING_DEVICE_PANEL.store(false, Ordering::Relaxed);
    result
}

type BuildPanelFn = unsafe extern "system" fn(usize, usize, u8) -> usize;

unsafe extern "system" fn build_panel_impl(a: usize, b: usize, slot: u8) -> usize {
    let slot = if MAKING_DEVICE_PANEL.load(Ordering::Relaxed) { DEVICE_SLOT } else { slot };
    unsafe { orig::<BuildPanelFn>(&ORIG_BUILD_PANEL)(a, b, slot) }
}

/// What the material menu's text function reads: the slot (+0), the material's param row (+0x10), the material
/// (+0x20).
#[repr(C)]
struct MaterialInfo {
    slot: u8,
    unused_8: usize,
    row: usize,
    unused_18: usize,
    material: u32,
}

type MaterialTextFn = unsafe extern "system" fn(usize, u32) -> usize;

unsafe extern "system" fn material_text_impl(out: usize, material: u32) -> usize {
    if !DEVICE_MATERIALS.contains(&material) {
        return unsafe { orig::<MaterialTextFn>(&ORIG_MATERIAL_TEXT)(out, material) };
    }
    let material_row: unsafe extern "system" fn(u32) -> usize = unsafe { std::mem::transmute(MATERIAL_ROW_ADDR) };
    type MenuTextFn = unsafe extern "system" fn(*const MaterialInfo, usize) -> usize;
    let menu_text: MenuTextFn = unsafe { std::mem::transmute(MENU_MATERIAL_TEXT_ADDR) };
    let row = unsafe { material_row(material) };
    let info = MaterialInfo { slot: DEVICE_SLOT, unused_8: 0, row, unused_18: 0, material };
    unsafe { menu_text(&info, out) }
}

/// The hooks, for lib.rs's hook table.
pub(crate) fn hooks() -> [Hook; 4] {
    [
        // movzx eax,byte [rcx] / cmp al,5
        (0x14045de80, [0x0f, 0xb6, 0x01, 0x3c, 0x05, 0x74], is_normal_slot_impl as *const (), &ORIG_IS_NORMAL_SLOT),
        // mov rax,rsp / mov [rax+10],rdx
        (0x140f43610, [0x48, 0x8b, 0xc4, 0x48, 0x89, 0x50], make_panel_impl as *const (), &ORIG_MAKE_PANEL),
        // mov [rsp+18],r8b / push rbp
        (0x140f49590, [0x44, 0x88, 0x44, 0x24, 0x18, 0x55], build_panel_impl as *const (), &ORIG_BUILD_PANEL),
        // push rbp / push rsi / push rdi / sub rsp,..
        (0x140e0cc30, [0x40, 0x55, 0x56, 0x57, 0x48, 0x81], material_text_impl as *const (), &ORIG_MATERIAL_TEXT),
    ]
}
