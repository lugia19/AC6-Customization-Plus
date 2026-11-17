//! The player's paint block: [[[GameDataMan]+8]+0x788] + 0x14C (the Paint screen edits it in place). Null while
//! loading, so the chain is read with checked reads.

use crate::read;

// GameDataMan's static pointer (the game's own getters read it as `mov rax,[this]` / `mov rax,[rax+8]`).
const GAME_DATA_MAN_ADDR: usize = 0x144e03a08;
const PLAYER_DATA_OFFSET: usize = 0x8;
const PAINT_DATA_OFFSET: usize = 0x788;
const PAINT_BLOCK_OFFSET: usize = 0x14c;

/// The value at `offset` in the player's paint block, if the chain resolves right now.
pub(crate) fn value<T: Copy + Default>(offset: usize) -> Option<T> {
    let game_data = read::<usize>(GAME_DATA_MAN_ADDR)?;
    let player = read::<usize>(game_data + PLAYER_DATA_OFFSET)?;
    let paint = read::<usize>(player + PAINT_DATA_OFFSET)?;
    read::<T>(paint + PAINT_BLOCK_OFFSET + offset)
}
