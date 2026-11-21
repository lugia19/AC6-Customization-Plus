//! The THRUSTERS row (rows.rs): the thruster color and the accent color (the ALLMIND type's flames), served to the
//! tinted copies (ext_values.rs: 900001, 900012), and the flame type (columns.rs). White until the player picks a color.

use crate::ext_values::{self, Block, Channels};
use crate::rows::{self, ROW_THRUSTER};
use crate::{columns, log};

// The row's choices: the generator's own type, then the custom types from columns::FIRST_CUSTOM_TYPE on.
const FLAME_TYPES: [&str; 5] = ["Generator's own", "Combustion", "Current", "Coral", "ALLMIND"];
const COLORS: usize = 2;
// The first external value of each color: the thruster color, the accent color.
const EXT_FIRST: [i32; COLORS] = [900001, 900012];
const COLOR_NAMES: [&str; COLORS] = ["flames", "accent"];

const DEFAULT_COLOR: u32 = 0xFFFFFF;
// ALLMIND's own mint.
const DEFAULT_ACCENT: u32 = 0x80FFD4;

static CHANNELS: [Channels; COLORS] = [const { ext_values::unset() }; COLORS];

pub(crate) fn claim_row() {
    rows::claim(ROW_THRUSTER, "THRUSTERS", (Some("Thruster color"), DEFAULT_COLOR),
        &[("Accent color (ALLMIND type only)", DEFAULT_ACCENT)], Some(("Flame type", &FLAME_TYPES, 0)));
}

pub(crate) fn ext_blocks() -> impl Iterator<Item = Block> {
    EXT_FIRST.into_iter().zip(&CHANNELS).map(|(first, channels)| Block { first, channels })
}

/// What was last applied, for the update loop (lib.rs).
pub(crate) struct Thrusters {
    colors: [Option<u32>; COLORS],
    flame_type: Option<u32>,
}

impl Thrusters {
    pub(crate) fn new() -> Self {
        for (channels, color) in CHANNELS.iter().zip([DEFAULT_COLOR, DEFAULT_ACCENT]) {
            ext_values::set_channels(channels, color);
        }
        Thrusters { colors: [None; COLORS], flame_type: None }
    }

    /// Applies the row's values when they change (none while the player's data isn't loaded).
    pub(crate) fn update(&mut self) {
        if let Some(choice) = rows::choice(ROW_THRUSTER).filter(|&choice| self.flame_type != Some(choice)) {
            self.flame_type = Some(choice);
            columns::set_flame_type(choice);
            log(&format!("thrusters: flame type {}", FLAME_TYPES.get(choice as usize).copied().unwrap_or("?")));
        }
        for (which, last) in self.colors.iter_mut().enumerate() {
            let Some(rgb) = rows::color(ROW_THRUSTER, which).filter(|&rgb| *last != Some(rgb)) else { continue };
            *last = Some(rgb);
            ext_values::set_channels(&CHANNELS[which], rgb);
            log(&format!("thrusters: {} #{rgb:06X}", COLOR_NAMES[which]));
        }
    }
}
