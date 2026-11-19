//! Feeds the custom generator flames (fxr/build_fxrs.mjs) the player's thruster color (the THRUSTER row, rows.rs)
//! through FXR external values, red, green, blue each: 900001-900003 what the additive copy of a flame adds,
//! 900004-900006 what the subtractive copy removes from the background (both copies play together; see set_channels),
//! 900007-900009 the plain color, for alpha-blended particles (smoke). Served as 1 + the amount (1..2). White until
//! the player picks a color.
//! 900012-900020 are the same nine for the accent color (the ALLMIND type's flames).
//!
//! Effects read external values through 0x142b56480, effectively `float *get(value_block, float *out, int id)`.
//!
//! The flame type: every generator-dependent effect (thruster flames at each thruster animation event, the core
//! expansions' effects) picks its column of the effect tables by 0x140829de0 `int color_type(chr_module)`, the
//! generator's thrustersColorType of the character at [chr_module + 8]. For the player's character the hook returns the
//! chosen flame type's custom column instead (build.py: 3 combustion, 4 current, 5 Coral, 6 ALLMIND, whose flames are
//! the tinted bundles); everyone else keeps their stock type.
//!
//! A polling thread caches the row's color and flame type for the hooks.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;

use crate::rows::{self, ROW_THRUSTER};
use crate::{Hook, log, orig};

// The first channel of each color: the thruster color, the accent color.
const EXT_FIRST_CHANNELS: [i32; 2] = [900001, 900012];
const CHANNEL_COUNT: usize = 9;
// The row's choices: the generator's own type, then the custom types from FIRST_CUSTOM_TYPE on.
const FLAME_TYPES: [&str; 5] = ["Generator's own", "Combustion", "Current", "Coral", "ALLMIND"];
const FIRST_CUSTOM_TYPE: i32 = 3;
const POLL_INTERVAL: Duration = Duration::from_millis(200);

const DEFAULT_COLOR: u32 = 0xFFFFFF;
// ALLMIND's own mint.
const DEFAULT_ACCENT: u32 = 0x80FFD4;

const WORLD_CHR_MAN_ADDR: usize = 0x144e0f598;
const PLAYER_INS_OFFSET: usize = 0xA5A0;

static ORIG_GET_EXT_VALUE: AtomicUsize = AtomicUsize::new(0);
static ORIG_COLOR_TYPE: AtomicUsize = AtomicUsize::new(0);
// Served values per color (add, remove, plain: red, green, blue each), as f32 bits (see set_channels).
static CHANNELS: [[AtomicU32; CHANNEL_COUNT]; 2] = [const { [const { AtomicU32::new(0) }; CHANNEL_COUNT] }; 2];
// The row's choice, an index into FLAME_TYPES.
static FLAME_TYPE: AtomicU32 = AtomicU32::new(0);


type GetExtValueFn = unsafe extern "system" fn(*mut u8, *mut u32, i32) -> *mut u32;

/// The external value read and color type hooks, for lib.rs's hook table.
pub(crate) fn hooks() -> [Hook; 2] {
    [
        // mov [rsp+18],r8d / mov [rsp+10],rdx
        (0x142b56480, [0x44, 0x89, 0x44, 0x24, 0x18, 0x48], get_ext_value_impl as *const (), &ORIG_GET_EXT_VALUE),
        // push rbx / sub rsp,30
        (0x140829de0, [0x40, 0x53, 0x48, 0x83, 0xec, 0x30], color_type_impl as *const (), &ORIG_COLOR_TYPE),
    ]
}

unsafe extern "system" fn color_type_impl(chr_module: usize) -> i32 {
    let stock = unsafe { orig::<unsafe extern "system" fn(usize) -> i32>(&ORIG_COLOR_TYPE)(chr_module) };
    if stock < 0 || player().is_none_or(|player| crate::read::<usize>(chr_module + 8) != Some(player)) {
        return stock;
    }
    match FLAME_TYPE.load(Ordering::Relaxed) as i32 {
        0 if stock < FIRST_CUSTOM_TYPE => FIRST_CUSTOM_TYPE + stock,
        0 => stock,
        choice => FIRST_CUSTOM_TYPE + choice - 1,
    }
}

/// The player's ChrIns (none while loading).
fn player() -> Option<usize> {
    let world_chr_man = crate::read::<usize>(WORLD_CHR_MAN_ADDR).filter(|&p| p != 0)?;
    crate::read::<usize>(world_chr_man + PLAYER_INS_OFFSET).filter(|&p| p != 0)
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
            FLAME_TYPE.store(choice as u32, Ordering::Relaxed);
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
