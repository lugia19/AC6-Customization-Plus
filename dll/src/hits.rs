//! The player's melee hits get tinted hit effects (docs/effect-columns.md). A hit's effects come from HitEffectSfxParam
//! by the attack and the surface, shared by every weapon of the same kind, enemies' included: so the swap goes by the
//! attack, not the ID.
//! - 0x140995870 `dispatch(dmg)` and 0x140995780 `dispatch_follow(list, dmg)` handle a hit's effects (on the DmgMan
//!   update's thread or its job workers): the hit's weapon, if it's one of the player's melee weapons, is kept in a
//!   thread-local while they run.
//! - 0x140b9f390 `list* build(out, ...)`, which they call, picks the effects ({i32 id; u8 follow}, 8 bytes, in an FD4
//!   vector): with a weapon kept, each effect that weapon has a copy of becomes the copy's bundle.

use std::cell::Cell;
use std::sync::atomic::AtomicUsize;

use crate::game;
use crate::{Hook, orig, weapons};

// The copies' IDs (tools/weapons.py): stock + model * MODEL_STRIDE + BUNDLE_OFFSET.
const MODEL_STRIDE: i32 = 100000;
const BUNDLE_OFFSET: i32 = 60000;
// The hit: its attacker (a character or a bullet), the BehaviorParam ID that launched it (100,000,000 + variation *
// 1000 + n for the player's), its bullet's ID (variation * 1000 + n; the stun baton's bullets have no behavior).
const DMG_ATTACKER_OFFSET: usize = 0x1e8;
const DMG_BEHAVIOR_OFFSET: usize = 0x64;
const DMG_BULLET_OFFSET: usize = 0x150;
const PLAYER_BEHAVIORS: i32 = 100_000_000;
// The list builder's vector: begin, end.
const LIST_BEGIN_OFFSET: usize = 8;
const LIST_END_OFFSET: usize = 0x10;
const ENTRY_SIZE: usize = 8;

type DispatchFn = unsafe extern "system" fn(usize) -> usize;
type DispatchFollowFn = unsafe extern "system" fn(usize, usize) -> usize;
type BuildFn = unsafe extern "system" fn(usize, u8, u8, u8, i8, i32, i32, u8) -> usize;

static ORIG_DISPATCH: AtomicUsize = AtomicUsize::new(0);
static ORIG_DISPATCH_FOLLOW: AtomicUsize = AtomicUsize::new(0);
static ORIG_BUILD: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    // The variation of the player's weapon making the hit being dispatched.
    static HITTING: Cell<Option<i32>> = const { Cell::new(None) };
}

pub(crate) fn hooks() -> [Hook; 3] {
    [
        // mov [rsp+8],rbx / mov [rsp+10],rsi
        (0x140995870, [0x48, 0x89, 0x5c, 0x24, 0x08, 0x48], dispatch_impl as *const (), &ORIG_DISPATCH),
        (0x140995780, [0x48, 0x89, 0x5c, 0x24, 0x08, 0x48], dispatch_follow_impl as *const (), &ORIG_DISPATCH_FOLLOW),
        // mov [rsp+20],r9b / mov [rsp+18],r8b
        (0x140b9f390, [0x44, 0x88, 0x4c, 0x24, 0x20, 0x44], build_impl as *const (), &ORIG_BUILD),
    ]
}

/// The variation of the weapon behind a hit, if the player made it.
fn players_variation(dmg: usize) -> Option<i32> {
    if !game::is_players(game::read::<usize>(dmg + DMG_ATTACKER_OFFSET)?) {
        return None;
    }
    let behavior = game::read::<i32>(dmg + DMG_BEHAVIOR_OFFSET)?;
    let bullet = game::read::<i32>(dmg + DMG_BULLET_OFFSET)?;
    if behavior >= PLAYER_BEHAVIORS {
        Some((behavior - PLAYER_BEHAVIORS) / 1000)
    } else {
        (bullet >= 0).then_some(bullet / 1000)
    }
}

/// Runs `dispatch` with the hit's weapon kept.
fn with_hit(dmg: usize, dispatch: impl FnOnce() -> usize) -> usize {
    let outer = HITTING.replace(players_variation(dmg));
    let result = dispatch();
    HITTING.set(outer);
    result
}

unsafe extern "system" fn dispatch_impl(dmg: usize) -> usize {
    with_hit(dmg, || unsafe { orig::<DispatchFn>(&ORIG_DISPATCH)(dmg) })
}

unsafe extern "system" fn dispatch_follow_impl(list: usize, dmg: usize) -> usize {
    with_hit(dmg, || unsafe { orig::<DispatchFollowFn>(&ORIG_DISPATCH_FOLLOW)(list, dmg) })
}

unsafe extern "system" fn build_impl(out: usize, material: u8, kind: u8, size: u8, offset: i8, concept1: i32,
                                     concept2: i32, mod100: u8) -> usize {
    let result = unsafe { orig::<BuildFn>(&ORIG_BUILD)(out, material, kind, size, offset, concept1, concept2, mod100) };
    if let Some(variation) = HITTING.get() {
        swap(out, variation);
    }
    result
}

fn swap(list: usize, variation: i32) -> Option<()> {
    let begin = game::read::<usize>(list + LIST_BEGIN_OFFSET)?;
    let end = game::read::<usize>(list + LIST_END_OFFSET)?;
    for entry in (begin..end).step_by(ENTRY_SIZE) {
        let sfx = game::read::<i32>(entry)?;
        if let Some(model) = weapons::hit_copy_model(variation, sfx) {
            unsafe { *(entry as *mut i32) = sfx + i32::from(model) * MODEL_STRIDE + BUNDLE_OFFSET };
        }
    }
    Some(())
}
