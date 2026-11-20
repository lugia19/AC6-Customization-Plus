//! The player's own columns in the generator-dependent effect tables. Every generator-dependent effect (thruster flames
//! at each thruster animation event, the core expansions' effects, melee weapons' effects) picks its column of the
//! effect tables by 0x140829de0 `int color_type(chr_module)`, the generator's thrustersColorType of the character at
//! [chr_module + 8]. For the player's character the hook returns a custom column instead (build.py: 3 combustion,
//! 4 current, 5 Coral, 6 ALLMIND), which plays the tinted copies; everyone else keeps their stock type. Which custom
//! column: the GENERATOR row's flame type (generator.rs).
//!
//! Bullets spawn their effects by plain ID, so the bullet effect spawn 0x140698380 is hooked too: the player's
//! bullets' effects with custom columns go through the table with the player's column.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::game::{self, CHR_HANDLE_OFFSET, CHR_MODULES_OFFSET};
use crate::{Hook, orig};

pub(crate) const FIRST_CUSTOM_TYPE: i32 = 3;
// The bullet's owner, as a character handle.
const BULLET_OWNER_OFFSET: usize = 0xce0;
// The replace table rows with custom columns (build.py EXPANSION_ROWS and WEAPON_EFFECTS): the core expansions' and
// the melee weapons' effects (assault armor's blast and light waves are bullets).
const CUSTOM_COLUMN_EFFECTS: std::ops::Range<i32> = 990_000..1_000_000;
// `int replace(color_type, sfx)`: the effect's GeneratorTypeSfxIdRplace_PC column for the type.
const REPLACE_ADDR: usize = 0x1415ce050;

type ColorTypeFn = unsafe extern "system" fn(usize) -> i32;
type ReplaceFn = unsafe extern "system" fn(i32, i32) -> i32;
type BulletEffectFn = unsafe extern "system" fn(usize, usize, i32, usize, usize) -> usize;

static ORIG_COLOR_TYPE: AtomicUsize = AtomicUsize::new(0);
static ORIG_BULLET_EFFECT: AtomicUsize = AtomicUsize::new(0);
// The flame type: 0 the generator's own, else custom type FIRST_CUSTOM_TYPE + it - 1.
static FLAME_TYPE: AtomicU32 = AtomicU32::new(0);

pub(crate) fn hooks() -> [Hook; 2] {
    [
        // push rbx / sub rsp,30
        (0x140829de0, [0x40, 0x53, 0x48, 0x83, 0xec, 0x30], color_type_impl as *const (), &ORIG_COLOR_TYPE),
        // mov rax,rsp / push rbp / push rsi / push rdi
        (0x140698380, [0x48, 0x8b, 0xc4, 0x55, 0x56, 0x57], bullet_effect_impl as *const (), &ORIG_BULLET_EFFECT),
    ]
}

pub(crate) fn set_flame_type(choice: u32) {
    FLAME_TYPE.store(choice, Ordering::Relaxed);
}

unsafe extern "system" fn color_type_impl(chr_module: usize) -> i32 {
    let stock = unsafe { orig::<ColorTypeFn>(&ORIG_COLOR_TYPE)(chr_module) };
    if stock < 0 || game::player().is_none_or(|player| game::read::<usize>(chr_module + 8) != Some(player)) {
        return stock;
    }
    players_column(stock)
}

/// The player's column for their generator's stock color type: the chosen flame type's.
fn players_column(stock: i32) -> i32 {
    match FLAME_TYPE.load(Ordering::Relaxed) as i32 {
        0 if stock < FIRST_CUSTOM_TYPE => FIRST_CUSTOM_TYPE + stock,
        0 => stock,
        choice => FIRST_CUSTOM_TYPE + choice - 1,
    }
}

unsafe extern "system" fn bullet_effect_impl(bullet: usize, out: usize, sfx: i32, pos: usize, rot: usize) -> usize {
    let sfx = if CUSTOM_COLUMN_EFFECTS.contains(&sfx) { players_bullet_effect(bullet, sfx).unwrap_or(sfx) } else { sfx };
    unsafe { orig::<BulletEffectFn>(&ORIG_BULLET_EFFECT)(bullet, out, sfx, pos, rot) }
}

fn players_bullet_effect(bullet: usize, sfx: i32) -> Option<i32> {
    let player = game::player()?;
    if game::read::<u64>(bullet + BULLET_OWNER_OFFSET)? != game::read::<u64>(player + CHR_HANDLE_OFFSET)? {
        return None;
    }
    let module = game::read::<usize>(game::read::<usize>(player + CHR_MODULES_OFFSET)?)?;
    let stock = unsafe { orig::<ColorTypeFn>(&ORIG_COLOR_TYPE)(module) };
    let replace: ReplaceFn = unsafe { game::game_fn(REPLACE_ADDR) };
    Some(unsafe { replace(players_column(stock), sfx) }).filter(|&id| id >= 0)
}
