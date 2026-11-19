//! Feeds the custom generator flames (fxr/build_fxrs.mjs) the player's thruster color (the THRUSTER row, rows.rs)
//! through FXR external values, red, green, blue each: 900001-900003 what the additive copy of a flame adds,
//! 900004-900006 what the subtractive copy removes from the background (both copies play together; see set_channels),
//! 900007-900009 the plain color, for alpha-blended particles (smoke). Served as 1 + the amount (1..2), so 0 keeps
//! meaning "no DLL" (the flames' wrappers then play stock). White until the player picks a color.
//! 900012-900020 are the same nine for the accent color (the ALLMIND type's assault boost).
//! 900011 is the row's choice, the flame type: 0 the generator's own, 1 combustion, 2 current, 3 Coral, 4 ALLMIND
//! (current plus the boss's effects); a flame's wrapper then plays its effect row's flame for that type.
//!
//! Effects read external values through 0x142b56480, effectively `float *get(value_block, float *out, int id)`.
//!
//! A polling thread caches the row's color and flame type for the hook.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;

use crate::rows::{self, ROW_THRUSTER};
use crate::{Hook, log, orig};

// The first channel of each color: the thruster color, the accent color.
const EXT_FIRST_CHANNELS: [i32; 2] = [900001, 900012];
const CHANNEL_COUNT: usize = 9;
const EXT_FLAME_TYPE: i32 = 900011;
// The row's choices; the index is what 900011 serves.
const FLAME_TYPES: [&str; 5] = ["Generator's own", "Combustion", "Current", "Coral", "ALLMIND"];
const POLL_INTERVAL: Duration = Duration::from_millis(200);

const DEFAULT_COLOR: u32 = 0xFFFFFF;
// ALLMIND's own mint.
const DEFAULT_ACCENT: u32 = 0x80FFD4;

static ORIG_GET_EXT_VALUE: AtomicUsize = AtomicUsize::new(0);
// Served values per color (add, remove, plain: red, green, blue each), as f32 bits (see set_channels).
static CHANNELS: [[AtomicU32; CHANNEL_COUNT]; 2] = [const { [const { AtomicU32::new(0) }; CHANNEL_COUNT] }; 2];
// Served value for 900011, as f32 bits.
static FLAME_TYPE: AtomicU32 = AtomicU32::new(0);


type GetExtValueFn = unsafe extern "system" fn(*mut u8, *mut u32, i32) -> *mut u32;

/// The external value read hook, for lib.rs's hook table.
pub(crate) fn hook() -> Hook {
    // mov [rsp+18],r8d / mov [rsp+10],rdx
    (0x142b56480, [0x44, 0x89, 0x44, 0x24, 0x18, 0x48], get_ext_value_impl as *const (), &ORIG_GET_EXT_VALUE)
}

/// Called for every external value read of every effect. Our IDs don't need the game's lookup at all (nothing else
/// provides them), so it's skipped for them.
unsafe extern "system" fn get_ext_value_impl(block: *mut u8, out: *mut u32, id: i32) -> *mut u32 {
    match served(id) {
        Some(value) if !out.is_null() => {
            unsafe { *out = value.load(Ordering::Relaxed) };
            out
        }
        _ => unsafe { orig::<GetExtValueFn>(&ORIG_GET_EXT_VALUE)(block, out, id) },
    }
}

/// The value we serve for an external value ID, as f32 bits, if it's one of ours.
fn served(id: i32) -> Option<&'static AtomicU32> {
    if id == EXT_FLAME_TYPE {
        return Some(&FLAME_TYPE);
    }
    EXT_FIRST_CHANNELS.iter().zip(&CHANNELS).find_map(|(&first, channels)| channels.get(usize::try_from(id - first).ok()?))
}

/// sRGB (what the color picker shows) to linear light.
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// How bright a color counts as, from its brightest channel (0-1): 0 = dark (64/255 or less), 1 = bright (160/255 or
/// more), smoothly in between.
fn brightness(brightest: f32) -> f32 {
    let t = ((brightest * 255.0 - 64.0) / 96.0).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t) // smoothstep
}

/// Serves a 0xRRGGBB color as color `which` (0 thruster, 1 accent): for each channel c (0-1),
/// - add (first three): c for bright colors (what looked right in game); for dark ones its linear intensity, since the
///   flames multiply it into HDR colors up to ~50 (#050505 at 0.02 would still leave those above 1: white);
/// - remove (next three): what the color lacks (1 - c), for dark colors only (additive flames can't get darker than
///   their background, so dark red darkens it in green and blue);
/// - plain (last three): c.
///
/// Each is served as 1 + the value.
fn set_channels(which: usize, rgb: u32) {
    let channels = &CHANNELS[which];
    let red = (rgb >> 16 & 0xFF) as f32 / 255.0;
    let green = (rgb >> 8 & 0xFF) as f32 / 255.0;
    let blue = (rgb & 0xFF) as f32 / 255.0;
    let bright = brightness(red.max(green).max(blue));
    for (i, c) in [red, green, blue].into_iter().enumerate() {
        let linear = srgb_to_linear(c);
        let add = linear + (c - linear) * bright;
        let remove = (1.0 - c) * (1.0 - bright);
        let plain = c;
        channels[i].store((1.0 + add).to_bits(), Ordering::Relaxed);
        channels[3 + i].store((1.0 + remove).to_bits(), Ordering::Relaxed);
        channels[6 + i].store((1.0 + plain).to_bits(), Ordering::Relaxed);
    }
}

fn poll_loop() {
    let mut last = [None; 2];
    let mut last_choice = None;
    loop {
        std::thread::sleep(POLL_INTERVAL);
        if let Some(choice) = rows::choice(ROW_THRUSTER).filter(|choice| last_choice != Some(*choice)) {
            last_choice = Some(choice);
            FLAME_TYPE.store((choice as f32).to_bits(), Ordering::Relaxed);
            log(&format!("thrusters: flame type {}", FLAME_TYPES.get(choice as usize).copied().unwrap_or("?")));
        }
        for (which, last) in last.iter_mut().enumerate() {
            let Some(rgb) = rows::color(ROW_THRUSTER, which).filter(|rgb| *last != Some(*rgb)) else { continue };
            *last = Some(rgb);
            set_channels(which, rgb);
            log(&format!("thrusters: {} #{rgb:06X}", ["flames", "accent"][which]));
        }
    }
}

pub(crate) fn claim_row() {
    rows::claim(ROW_THRUSTER, "GENERATOR", (Some("Thruster color"), DEFAULT_COLOR), Some(("Accent color", DEFAULT_ACCENT)),
        Some(("Flame type", &FLAME_TYPES, 0)));
}

pub(crate) fn start() {
    set_channels(0, DEFAULT_COLOR);
    set_channels(1, DEFAULT_ACCENT);
    std::thread::spawn(poll_loop);
}
