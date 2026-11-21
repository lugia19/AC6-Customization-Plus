//! The game (1.09.1) as several modules read it: memory and string helpers, and the pointer chains they share.
//! Addresses only one module uses stay in that module.

unsafe extern "system" {
    fn GetCurrentProcess() -> isize;
    fn ReadProcessMemory(process: isize, address: usize, buffer: *mut u8, size: usize, read: *mut usize) -> i32;
}

/// Reads memory that may not be mapped (pointer chains while loading, effects being freed) without faulting.
pub(crate) fn read<T: Copy + Default>(addr: usize) -> Option<T> {
    let mut v = T::default();
    let mut n = 0;
    let ok = unsafe { ReadProcessMemory(GetCurrentProcess(), addr, &mut v as *mut T as *mut u8, size_of::<T>(), &mut n) };
    (ok != 0 && n == size_of::<T>()).then_some(v)
}

/// A game function at `address`, typed as `F` (an `unsafe extern "system" fn`).
pub(crate) unsafe fn game_fn<F: Copy>(address: usize) -> F {
    unsafe { std::mem::transmute_copy(&address) }
}

/// UTF-16 with a terminator, at compile time (ASCII only).
pub(crate) const fn utf16<const N: usize>(text: &[u8; N]) -> [u16; N] {
    let mut out = [0; N];
    let mut i = 0;
    while i < N {
        out[i] = text[i] as u16;
        i += 1;
    }
    out
}

/// UTF-16 with a terminator, kept forever: the game's labels hold the text's address.
pub(crate) fn leak_utf16(text: &str) -> &'static [u16] {
    text.encode_utf16().chain([0]).collect::<Vec<u16>>().leak()
}

// ---- characters ------------------------------------------------------------------------------------------------------

const WORLD_CHR_MAN_ADDR: usize = 0x144e0f598;
const PLAYER_INS_OFFSET: usize = 0xA5A0;
// ChrIns: [[+ 0x320]] is its module that the generator color type takes; its AC's model object (its part models).
pub(crate) const CHR_MODULES_OFFSET: usize = 0x320;
pub(crate) const CHR_MODEL_OFFSET: usize = 0x47f8;
// `ChrIns* character_of(FieldIns*)`: a character itself, a bullet's owner (through the bullets that fired it).
const CHARACTER_OF_ADDR: usize = 0x140b3c790;

/// The player's ChrIns (none while loading).
pub(crate) fn player() -> Option<usize> {
    let world_chr_man = read::<usize>(WORLD_CHR_MAN_ADDR).filter(|&p| p != 0)?;
    read::<usize>(world_chr_man + PLAYER_INS_OFFSET).filter(|&p| p != 0)
}

/// Whether a character or bullet (a FieldIns) is the player's: the player, or fired by the player.
pub(crate) fn is_players(field_ins: usize) -> bool {
    let character_of: unsafe extern "system" fn(usize) -> usize = unsafe { game_fn(CHARACTER_OF_ADDR) };
    field_ins != 0 && player().is_some_and(|player| unsafe { character_of(field_ins) } == player)
}

// ChrIns: its AC's assembly (what the AC parts loader builds it from); the assembly's weapons (EquipParamWeapon IDs,
// -1 for none) by slot: 0 left arm, 1 right arm, 2 left back, 3 right back (0x1406f5890 reads them).
const CHR_ASSEMBLY_OFFSET: usize = 0x47e0;
const ASSEMBLY_WEAPONS_OFFSET: usize = 0xf4;
pub(crate) const SLOT_LEFT_ARM: usize = 0;
pub(crate) const SLOT_LEFT_BACK: usize = 2;

/// The weapon the player's AC has in `slot` (the garage's AC too: the Paint screen paints it).
pub(crate) fn player_weapon(slot: usize) -> Option<i32> {
    let assembly = read::<usize>(player()? + CHR_ASSEMBLY_OFFSET).filter(|&a| a != 0)?;
    read::<i32>(assembly + ASSEMBLY_WEAPONS_OFFSET + slot * 4).filter(|&id| id >= 0)
}

// ---- the player's paint data -----------------------------------------------------------------------------------------

// [[[GameDataMan]+8]+0x788] + 0x14C (the Paint screen edits it in place); GameDataMan's static pointer (the game's own
// getters read it as `mov rax,[this]` / `mov rax,[rax+8]`). Null while loading: checked reads.
const GAME_DATA_MAN_ADDR: usize = 0x144e03a08;
const PLAYER_DATA_OFFSET: usize = 0x8;
const PAINT_DATA_OFFSET: usize = 0x788;
const PAINT_BLOCK_OFFSET: usize = 0x14c;

/// The value at `offset` in the player's paint block, if the chain resolves right now.
pub(crate) fn paint_value<T: Copy + Default>(offset: usize) -> Option<T> {
    let game_data = read::<usize>(GAME_DATA_MAN_ADDR)?;
    let player = read::<usize>(game_data + PLAYER_DATA_OFFSET)?;
    let paint = read::<usize>(player + PAINT_DATA_OFFSET)?;
    read::<T>(paint + PAINT_BLOCK_OFFSET + offset)
}
