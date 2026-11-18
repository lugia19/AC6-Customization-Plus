//! Brings back the DEVICE row's brightness setting in the Paint screen.
//!
//! The game has it all, switched off at the row: DEVICE's material (materials 90-99, "Light Source Intensity", per
//! part like any material; the default is 92) drives how bright the AC's lights are, and the stock material picker
//! lists those ten for DEVICE's slot, labelled by their glow. But the DEVICE row is built color-only, so nothing leads
//! to it. rows.rs makes the row a normal one (is-normal-slot) and gives it a panel (panel.rs): the game's Color entry
//! and a Brightness entry opening that picker.

use std::sync::OnceLock;

use crate::ui::leak_utf16;

pub(crate) const SLOT: u8 = 5;

pub(crate) fn title() -> &'static [u16] {
    static TITLE: OnceLock<&'static [u16]> = OnceLock::new();
    TITLE.get_or_init(|| leak_utf16("Brightness"))
}
