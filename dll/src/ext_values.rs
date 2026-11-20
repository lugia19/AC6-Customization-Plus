//! The FXR external values the mod serves: its colors, for the effect copies the build tints with them
//! (fxr/build_fxrs.mjs). Effects read external values through 0x142b56480, effectively `float *get(value_block,
//! float *out, int id)`; the hook answers our IDs itself.
//!
//! A color is served on nine consecutive IDs (a block), red, green, blue each: what the additive copy of an effect
//! adds, what the subtractive copy removes from the background (both play together), and the plain color for
//! alpha-blended particles (see set_channels). Each feature registers its blocks before the hooks go live (lib.rs).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::{Hook, orig};

pub(crate) const CHANNEL_COUNT: usize = 9;
/// A block's served values, as f32 bits.
pub(crate) type Channels = [AtomicU32; CHANNEL_COUNT];

/// A color served from external value `first` on.
pub(crate) struct Block {
    pub(crate) first: i32,
    pub(crate) channels: &'static Channels,
}

static BLOCKS: OnceLock<Vec<Block>> = OnceLock::new();
static ORIG_GET_EXT_VALUE: AtomicUsize = AtomicUsize::new(0);

type GetExtValueFn = unsafe extern "system" fn(*mut u8, *mut u32, i32) -> *mut u32;

pub(crate) const fn unset() -> Channels {
    [const { AtomicU32::new(0) }; CHANNEL_COUNT]
}

pub(crate) fn register(blocks: Vec<Block>) {
    assert!(BLOCKS.set(blocks).is_ok(), "external value blocks registered twice");
}

pub(crate) fn hooks() -> [Hook; 1] {
    // mov [rsp+18],r8d / mov [rsp+10],rdx
    [(0x142b56480, [0x44, 0x89, 0x44, 0x24, 0x18, 0x48], get_ext_value_impl as *const (), &ORIG_GET_EXT_VALUE)]
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

fn served(id: i32) -> Option<&'static AtomicU32> {
    BLOCKS.get()?.iter().find_map(|block| block.channels.get(usize::try_from(id - block.first).ok()?))
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

/// 0xRRGGBB as red, green, blue (0-1).
pub(crate) fn channels(rgb: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| (rgb >> shift & 0xFF) as f32 / 255.0)
}

/// Serves a 0xRRGGBB color on a block: for each channel c (0-1),
/// - add (first three): c for bright colors (what looked right in game); for dark ones its linear intensity, since the
///   flames multiply it into HDR colors up to ~50 (#050505 at 0.02 would still leave those above 1: white);
/// - remove (next three): what the color lacks (1 - c), for dark colors only (additive flames can't get darker than
///   their background, so dark red darkens it in green and blue);
/// - plain (last three): c.
///
/// Each is served as 1 + the value.
pub(crate) fn set_channels(served: &Channels, rgb: u32) {
    let rgb = channels(rgb);
    let bright = brightness(rgb.iter().copied().fold(0.0, f32::max));
    for (i, c) in rgb.into_iter().enumerate() {
        let linear = srgb_to_linear(c);
        let add = linear + (c - linear) * bright;
        let remove = (1.0 - c) * (1.0 - bright);
        let plain = c;
        served[i].store((1.0 + add).to_bits(), Ordering::Relaxed);
        served[3 + i].store((1.0 + remove).to_bits(), Ordering::Relaxed);
        served[6 + i].store((1.0 + plain).to_bits(), Ordering::Relaxed);
    }
}
