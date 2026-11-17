//! Brings back the DEVICE row's brightness setting in the Paint screen.
//!
//! The game has it all, switched off at the row: DEVICE's material (materials 90-99, "Light Source Intensity", per
//! part like any material; the default is 92) drives how bright the AC's lights are, and the material menu lists
//! those ten for DEVICE. But the DEVICE row is built color-only, so nothing leads to that menu.
//!
//! No hooks of its own: rows.rs hooks the same functions for the extra rows and asks here.
//! - is-normal-slot: true for DEVICE, so its row is built like MAIN..OTHER: confirming it opens the Color / Material
//!   panel instead of the color picker.
//! - row kind -> panel: the game's switch has no case for DEVICE; rows.rs makes the panel for its slot.
//! - material text, in the menu and on the panel: the game's own is "Light Source Intensity: 1.5" in the menu and
//!   nothing fitting on the panel ("Reflectiveness: x  Luster: y"); both get "Brightness 150%".

use std::sync::OnceLock;

pub(crate) const SLOT: u8 = 5;
const FIRST_MATERIAL: u32 = 90;
const MATERIAL_COUNT: usize = 10;
// Material -> its param row (PlayerMaterialPresetParam); its glow, a float, is at +8: 1.0 for the default material.
const MATERIAL_ROW_ADDR: usize = 0x140e0cdd0;
const ROW_GLOW_OFFSET: usize = 8;

// UTF-16 with a terminator, made on first use and kept: the game's labels hold the text's address.
static TEXTS: [OnceLock<Vec<u16>>; MATERIAL_COUNT] = [const { OnceLock::new() }; MATERIAL_COUNT];

/// The text for one of DEVICE's materials: its brightness, in percent of the default's. None for other materials.
pub(crate) fn material_text(material: u32) -> Option<&'static [u16]> {
    let text = TEXTS.get(material.checked_sub(FIRST_MATERIAL)? as usize)?;
    Some(text.get_or_init(|| {
        let material_row: unsafe extern "system" fn(u32) -> usize = unsafe { std::mem::transmute(MATERIAL_ROW_ADDR) };
        let row = unsafe { material_row(material) };
        let glow = crate::read::<f32>(row + ROW_GLOW_OFFSET).unwrap_or(1.0);
        format!("Brightness {}%", (glow * 100.0).round() as u32).encode_utf16().chain([0]).collect()
    }))
}
