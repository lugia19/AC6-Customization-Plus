# AC6-Customization-Plus

More to customize in Armored Core VI's Paint screen (1.09.1), saved with each AC:

- **Thruster color and flame type.** Your AC's thruster flames take the color: boosting, quick boosting, assault
  boosting, all of it. The type picks the flame itself, whatever generator you have.
- **Expansion color** for Assault Armor, Pulse Armor, Pulse Protection and Terminal Armor.
- **HUD color, warning color and opacity** for the in-mission HUD.
- **Light brightness**, a setting the game has but hides.
- **Melee weapon colors**: your melee weapons' blades and effects in colors of your own.

## Use

1. Grab the zip from [Releases](../../releases) (or build it, below), extract it anywhere, and launch
   `customization-plus.me3` with [me3](https://github.com/garyttierney/me3).
   It uses its own save, `CustomizationPlus.sl2`, which me3 copies from your real save on first launch.
2. In the garage, Paint: below WEATHERING there's a new panel, ENTIRE AC, with two rows. Each is one setting for
   the whole AC, whichever part tab is open. Confirming a row opens its panel, like MAIN's Color / Material.

**GENERATOR**: a thruster color, an accent color, a flame type and an expansion color. Until you pick a color, it's
white. Dark colors work too: the flames darken what's behind them instead of glowing. The accent color is for the
ALLMIND type; the expansion color is your core expansion's (its effects follow the flame type: the Coral type gets the
Coral version). The types:
- **Generator's own** (the default): the flames of the generator you have equipped.
- **Combustion**, **Current**, **Coral**: the game's three kinds of flame. They differ in shape and detail, not only
  in color: Current has its own flame model, Coral adds sparkles and haze.
- **ALLMIND**: two-tone flames in the style of ALLMIND's: every flame starts in the accent color and turns into the
  thruster color. Boosting adds the boss's spark spray, steady light and start flash, quick boosts are the boss's
  own, and the held assault boost is the burst of one of its special moves.

The exhaust fumes stay as the game has them.

**HUD** is for the in-mission HUD:
- **Color**: the HUD's color.
- **Opacity**: 0% up to 100%.
- **Warning color**: the color of warnings. Every state color follows it: red becomes this color, and orange,
  yellow, green and the rest keep their distance from it.

The defaults (the stock pale cyan, 100%, red) are the stock HUD. They apply from the next sortie (AC TEST counts).

**DEVICE** gets its hidden setting back: how bright the AC's lights are. Confirming the DEVICE row now opens a
panel with its color and a Brightness entry, which opens the game's own material picker with the ten light
intensities (Brightness 10% to 450%), per part like any material. NPC ACs use them; the stock menu just never let you reach
them.

Coming from a build with the HUD WARNING row: the warning color is now in the HUD row's panel, and a warning color
picked before isn't carried over.

Everything is stored in the AC's paint data, so it's saved with the AC and carried by designs.

**WEAPON VFX** is for your melee weapons: its panel shows the melee weapon on your left arm and the one in your left
weapon bay, each with its color; like the game's own rows, the preview flashes the weapon you're on. Every melee weapon
keeps its own color on the AC, so swapping weapons shows each one's color, and dark colors work as they do for the
thrusters. Covered: the energy blades (HI-32: BU-TT/A, Vvc-770LB, WLT 101, VP-67LD, MOONLIGHT, ML-REDSHIFT,
Vvc-774LS, VE-67LLA: blade or glow and effects, light waves included), the HMMR plasma thrower (its chain and effects)
and the VP-67EB stun baton (effects). Only your AC changes; enemies with the same weapon keep the stock look. Without a
color, a weapon keeps its stock look, and its effects get a color close to their stock one.

Coming from 0.1.0 (AC6-Thruster-Customization)? The save file has a new name: rename `PaintMenu.sl2` to
`CustomizationPlus.sl2` in your save folder to keep what you did there.

## Known issues

- The Paint screen's pattern thumbnails can break after going in and out of it, or after a mission: the PATTERN
  thumbnail stops updating, or some parts' swatches show no pattern. The AC itself keeps it. That's an me3 bug with
  overridden menu files, not this mod (it happens without the DLL too):
  [me3#902](https://github.com/garyttierney/me3/issues/902).
- The new rows' labels and choices are in English in every language.
- The package replaces whole game files (`regulation.bin`, the common effects, the material archive, the shader
  bundle, two menu movies), so it doesn't combine with other mods that replace the same ones.
- The assembly preview shows the stock blades (it's a separate model, not your AC).

## How it works

A DLL and a package of game files built from your install. In short:

- **Paint UI** ([paint-ui.md](docs/paint-ui.md)): the rows are list items the DLL adds to the list the game builds,
  with panels made from the game's own menu pieces. DEVICE's brightness reuses the game's own material menu.
- **Storage** ([storage.md](docs/storage.md)): the rows keep their values in parts of the AC's paint data the game
  never uses.
- **Effect columns** ([effect-columns.md](docs/effect-columns.md)): the game picks thruster, expansion and melee
  weapon effects from tables by the generator's color type; the DLL answers with columns of their own for your AC only.
- **Tinted copies** ([tinted-copies.md](docs/tinted-copies.md)): those columns play copies of the effects, colored by
  values the DLL serves.
- **Model copies** ([model-copies.md](docs/model-copies.md)): effect models colored by their material get neutral
  copies the tint can color.
- **Material overrides** ([material-overrides.md](docs/material-overrides.md)): energy blades are a material of the
  weapon's model, shared by every AC; the DLL gives your weapon models their own colors for it, the way paint colors
  each AC's parts.
- **HUD** ([hud.md](docs/hud.md)): each sortie, the DLL hands the game a copy of the HUD movie with every color
  rewritten.

Without the DLL, everything plays stock. Which feature uses what, the hooks and the build:
[docs/overview.md](docs/overview.md).

## Build

```
python build.py --game "<...>/ARMORED CORE VI FIRES OF RUBICON/Game" --witchy "<...>/WitchyBND.exe"
```

Needs Python 3, node/npm, cargo (stable) and [WitchyBND](https://github.com/ividyon/WitchyBND) 3.x. It writes
`package/`: the mod folder, `customization_plus.dll` and `customization-plus.me3`.

WitchyBND 3.0.1 refuses to repack 1.09.1 regulations out of the box: raise the version in its
`Assets/Paramdex/AC6/Upgrader/version.txt` to `1_09_1_0042L`.
