//! WEAPON VFX: the player's melee weapons in colors of their own, one per weapon model, a global setting.
//! - Effects: each weapon effect has a replace row whose custom columns (the player's only: columns.rs's color type
//!   hook) play a copy tinted by external values (build.py WEAPON_EFFECTS): a weapon's color on a block from EXT_BASE +
//!   10 * its model (ext_values.rs).
//! - Blades that are a material of the part (build.py WEAPON_BLADE_MATERIALS): the player's AC loads a copy of the part,
//!   model 0xxx -> 9xxx, whose blade material is its own copy. Hooks: the AC parts loader 0x1410d8460 notes whether
//!   the player's parts object is loading (a thread-local: the name builder runs inside it), the part name builder
//!   0x1415d0080 then renames. The copied materials are found by name in the game's
//!   material repository (MtdRepository) and their colors set like the game's own matbin reload does: the value, then a
//!   commit that rebuilds the material's constant buffer.
//! Colors: customization_plus_weapons.txt next to the DLL, "<model> = RRGGBB" per line, reread when it changes. A
//! weapon without one keeps its stock blade and its effects get the weapon's default color.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;

use crate::ext_values::{self, Block, Channels};
use crate::game::{self, CHR_PARTS_OFFSET, game_fn, utf16};
use crate::{Hook, log, orig};

struct Weapon {
    model: u16,
    name: &'static str,
    /// What its effects get until a color is set.
    default: u32,
    /// Its blade material's copy (build.py player_copy), if the blade is a material of the part.
    blade: Option<&'static [u16]>,
}

const WEAPONS: [Weapon; 10] = [
    Weapon { model: 211, name: "pulse blade", default: 0x99FFFF, blade: Some(&utf16(b"P[WP_9210]_Laser\0")) },
    Weapon { model: 221, name: "laser blade", default: 0x4DB3FF, blade: Some(&utf16(b"P[WP_9220]_Laser\0")) },
    Weapon { model: 230, name: "WLT 101", default: 0xFF4D4D, blade: None },
    Weapon { model: 241, name: "laser dagger", default: 0x4DB3FF, blade: Some(&utf16(b"P[WP_9240]_Laser\0")) },
    Weapon { model: 250, name: "MOONLIGHT", default: 0x80FFCC, blade: Some(&utf16(b"P[WP_9250]_Laser\0")) },
    Weapon { model: 251, name: "ML-REDSHIFT", default: 0xFF4D4D, blade: Some(&utf16(b"P[WP_9251]_Laser\0")) },
    Weapon { model: 411, name: "laser slicer", default: 0x66CCFF, blade: Some(&utf16(b"P[WP_9410]_Laser\0")) },
    Weapon { model: 610, name: "laser lance", default: 0x4DB3FF, blade: Some(&utf16(b"P[WP_9610]_Laser\0")) },
    Weapon { model: 910, name: "stun baton", default: 0xFFCC4D, blade: None },
    Weapon { model: 1210, name: "plasma thrower", default: 0x7766FF, blade: None },
];
// A weapon's nine external values start at EXT_BASE + EXT_BLOCK * model (build.py reads EXT_BASE).
const EXT_BASE: i32 = 900000;
const EXT_BLOCK: i32 = 10;
const BLADE_COLORS: [&[u16]; 3] = [
    &utf16(b"P_LaserBlade__001_snp_0_color_6\0"),
    &utf16(b"P_LaserBlade__001_snp_0_color_8\0"),
    &utf16(b"P_LaserBlade__001_snp_0_color_10\0"),
];
// A color parameter's rgba (then its intensity).
const PARAM_VALUE_OFFSET: usize = 0x40;

const SETTINGS_FILE: &str = "customization_plus_weapons.txt";

const MTD_REPOSITORY_ADDR: usize = 0x144e1cc38;
// `MtdResCap* find(repository, const wchar_t* name)`; the material at rescap + 0x78.
const FIND_MATERIAL_ADDR: usize = 0x141344fe0;
const RESCAP_MATERIAL_OFFSET: usize = 0x78;
// `param* find_param(material, const wchar_t* name)`, `void commit(material)`.
const FIND_PARAM_ADDR: usize = 0x142300730;
const COMMIT_ADDR: usize = 0x1423020a0;
// The name builder's output, an FD4 wide string: +8 the characters (inline up to 7, else a pointer there), +0x18 the
// length, +0x20 the capacity.
const STRING_LENGTH_OFFSET: usize = 0x18;
const STRING_CAPACITY_OFFSET: usize = 0x20;

type FindFn = unsafe extern "system" fn(usize, *const u16) -> usize;
type CommitFn = unsafe extern "system" fn(usize);
type LoaderFn = unsafe extern "system" fn(usize, usize, usize, usize, usize, usize, usize, usize, usize) -> usize;
type PartNameFn = unsafe extern "system" fn(usize, usize, usize, usize, usize) -> usize;

static ORIG_LOADER: AtomicUsize = AtomicUsize::new(0);
static ORIG_PART_NAME: AtomicUsize = AtomicUsize::new(0);
static CHANNELS: [Channels; WEAPONS.len()] = [const { ext_values::unset() }; WEAPONS.len()];

thread_local! {
    // Whether the AC parts loader is loading the player's parts, while it runs.
    static LOADING_PLAYERS: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn hooks() -> [Hook; 2] {
    [
        // push rbp / push rsi / push rdi / push r12
        (0x1410d8460, [0x40, 0x55, 0x56, 0x57, 0x41, 0x54], loader_impl as *const (), &ORIG_LOADER),
        // mov [rsp+8],rbx / mov [rsp+10],rbp
        (0x1415d0080, [0x48, 0x89, 0x5c, 0x24, 0x08, 0x48], part_name_impl as *const (), &ORIG_PART_NAME),
    ]
}

unsafe extern "system" fn loader_impl(parts: usize, a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize,
                                      a8: usize, a9: usize) -> usize {
    let players = game::player().and_then(|player| game::read::<usize>(player + CHR_PARTS_OFFSET)) == Some(parts);
    let outer = LOADING_PLAYERS.replace(players);
    let result = unsafe { orig::<LoaderFn>(&ORIG_LOADER)(parts, a2, a3, a4, a5, a6, a7, a8, a9) };
    LOADING_PLAYERS.set(outer);
    result
}

/// Builds a part's name ("WP_L_0221", "WP_L_0221_L"...); the player's blade parts get their copy's (the number's
/// leading 0 -> 9).
unsafe extern "system" fn part_name_impl(out: usize, row: usize, a3: usize, a4: usize, a5: usize) -> usize {
    let result = unsafe { orig::<PartNameFn>(&ORIG_PART_NAME)(out, row, a3, a4, a5) };
    if LOADING_PLAYERS.get() {
        rename_blade_part(out);
    }
    result
}

fn rename_blade_part(string: usize) -> Option<()> {
    let length = game::read::<usize>(string + STRING_LENGTH_OFFSET)?;
    let inline = game::read::<usize>(string + STRING_CAPACITY_OFFSET)? <= 7;
    let at = if inline { string + 8 } else { game::read::<usize>(string + 8)? };
    let chars = game::read::<[u16; 10]>(at)?;
    let digits = String::from_utf16(chars.get(6..9)?).ok()?;
    let ends = length == 9 || length > 9 && chars[9] == u16::from(b'_');
    let model: u16 = digits.parse().ok()?;
    if chars[..6] != utf16(b"WP_L_0") || !ends || !WEAPONS.iter().any(|w| w.model == model && w.blade.is_some()) {
        return None;
    }
    unsafe { *((at + 5 * 2) as *mut u16) = u16::from(b'9') };
    Some(())
}

pub(crate) fn ext_blocks() -> impl Iterator<Item = Block> {
    WEAPONS.iter().zip(&CHANNELS).map(|(weapon, channels)| Block { first: EXT_BASE + EXT_BLOCK * i32::from(weapon.model), channels })
}

/// A copied blade's material and color parameters, with their stock rgb.
struct Blade {
    material: usize,
    params: Vec<(usize, [f32; 3])>,
}

fn find_blade(name: &[u16]) -> Option<Blade> {
    let repository = game::read::<usize>(MTD_REPOSITORY_ADDR).filter(|&r| r != 0)?;
    let rescap = Some(unsafe { game_fn::<FindFn>(FIND_MATERIAL_ADDR)(repository, name.as_ptr()) }).filter(|&r| r != 0)?;
    let material = game::read::<usize>(rescap + RESCAP_MATERIAL_OFFSET).filter(|&m| m != 0)?;
    let find_param: FindFn = unsafe { game_fn(FIND_PARAM_ADDR) };
    let params = BLADE_COLORS.iter().map(|color| {
        let param = Some(unsafe { find_param(material, color.as_ptr()) }).filter(|&p| p != 0)?;
        Some((param, game::read::<[f32; 3]>(param + PARAM_VALUE_OFFSET)?))
    });
    Some(Blade { material, params: params.collect::<Option<_>>()? })
}

/// Each blade color takes the chosen color, at its stock brightness (its brightest channel); stock without one.
fn paint_blade(blade: &Blade, rgb: Option<u32>) {
    for &(param, stock) in &blade.params {
        let value = rgb.map_or(stock, |rgb| {
            let brightest = stock.iter().copied().fold(0.0, f32::max);
            ext_values::channels(rgb).map(|c| c * brightest)
        });
        unsafe { *((param + PARAM_VALUE_OFFSET) as *mut [f32; 3]) = value };
    }
    unsafe { game_fn::<CommitFn>(COMMIT_ADDR)(blade.material) };
}

/// The settings file's colors per model ("221 = 4DB3FF"; '#' starts a comment; a later line wins).
fn read_colors(text: &str) -> BTreeMap<u16, u32> {
    text.lines()
        .filter_map(|line| {
            let (model, color) = line.split('#').next()?.split_once('=')?;
            Some((model.trim().parse().ok()?, u32::from_str_radix(color.trim(), 16).ok()?))
        })
        .collect()
}

/// The chosen colors (the settings file, until the WEAPON VFX row).
fn current_colors(path: &std::path::Path) -> BTreeMap<u16, u32> {
    read_colors(&std::fs::read_to_string(path).unwrap_or_default())
}

/// What was last applied, for the update loop (lib.rs).
pub(crate) struct Weapons {
    path: PathBuf,
    // The blades' materials, once found (they load at boot), in WEAPONS' order.
    blades: [Option<Blade>; WEAPONS.len()],
    colors: Option<BTreeMap<u16, u32>>,
}

impl Weapons {
    pub(crate) fn new() -> Self {
        let path = crate::module_dir().join(SETTINGS_FILE);
        if !path.exists() {
            let lines: String = WEAPONS.iter().map(|w| format!("# {} = {:06X}  ({})\n", w.model, w.default, w.name)).collect();
            let _ = std::fs::write(&path, format!("# WEAPON VFX colors: <model> = RRGGBB\n{lines}"));
        }
        Weapons { path, blades: [const { None }; WEAPONS.len()], colors: None }
    }

    /// Applies the colors when they change, and to each blade material when it's found.
    pub(crate) fn update(&mut self) {
        let now = current_colors(&self.path);
        let changed = self.colors.as_ref() != Some(&now);
        if changed {
            let listed: Vec<String> = now.iter().map(|(model, color)| format!("{model} #{color:06X}")).collect();
            log(&format!("weapons: colors {}", listed.join(", ")));
            for (weapon, channels) in WEAPONS.iter().zip(&CHANNELS) {
                ext_values::set_channels(channels, now.get(&weapon.model).copied().unwrap_or(weapon.default));
            }
        }
        for (weapon, slot) in WEAPONS.iter().zip(&mut self.blades) {
            let Some(name) = weapon.blade else { continue };
            if slot.is_none() {
                *slot = find_blade(name);
                if let Some(blade) = slot {
                    log(&format!("weapons: blade material of {} at {:#x}", weapon.model, blade.material));
                    paint_blade(blade, now.get(&weapon.model).copied());
                }
            } else if changed {
                paint_blade(slot.as_ref().unwrap(), now.get(&weapon.model).copied());
            }
        }
        self.colors = Some(now);
    }
}
