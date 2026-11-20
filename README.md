# AC6-Customization-Plus

More to customize in Armored Core VI's Paint screen (1.09.1), saved with each AC:

- **Thruster color and flame type.** Your AC's thruster flames take the color: boosting, quick boosting, assault
  boosting, all of it. The type picks the flame itself, whatever generator you have.
- **Expansion color** for Assault Armor, Pulse Armor, Pulse Protection and Terminal Armor.
- **HUD color, warning color and opacity** for the in-mission HUD.
- **Light brightness**, a setting the game has but hides.
- **Melee weapon colors** (in progress, no menu yet): each melee weapon in a color of its own, blade and effects.

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

**Melee weapon colors** don't have their row yet: for now they're set in `customization_plus_weapons.txt` next to
`customization_plus.dll` (written on first launch, listing every weapon). One line per weapon model, e.g.
`221 = FF00FF` makes the Vvc-770LB laser blade magenta; the game picks up a saved change on the next swing. Covered:
the energy blades (pulse blade, laser blade, WLT 101, laser dagger, MOONLIGHT, ML-REDSHIFT, laser slicer, laser
lance: blade and effects, light waves included) and the stun baton and plasma thrower (effects). Only your AC changes;
enemies with the same weapon keep the stock look. A weapon without a color keeps its stock blade, and its effects get a
color close to its stock one. These colors are one per weapon for every AC, not saved with the AC.

Coming from 0.1.0 (AC6-Thruster-Customization)? The save file has a new name: rename `PaintMenu.sl2` to
`CustomizationPlus.sl2` in your save folder to keep what you did there.

## Known issues

- The Paint screen's pattern thumbnails can break after going in and out of it, or after a mission: the PATTERN
  thumbnail stops updating, or some parts' swatches show no pattern. The AC itself keeps it. That's an me3 bug with
  overridden menu files, not this mod (it happens without the DLL too):
  [me3#902](https://github.com/garyttierney/me3/issues/902).
- The new rows' labels and choices are in English in every language.
- The package replaces whole game files (`regulation.bin`, the common effects, the material archive, two menu
  movies), so it doesn't combine with other mods that replace the same ones.
- The assembly preview shows the stock blades (it's a separate model, not your AC).

## How it works

- **Paint rows.** The rows are list items the DLL adds to the list the game builds, shown by clips added to the Paint
  movie, with panels built from the game's own menu pieces; a row keeps its values in a part of the AC's paint data the
  game never uses. DEVICE's brightness reuses the game's own material menu.
  [docs/paint-rows.md](docs/paint-rows.md)
- **Flames and expansions.** The game picks thruster and expansion effects by the generator's color type; the DLL
  answers with custom columns for your AC only, which play copies of the effects tinted with colors the DLL serves.
  [docs/effects.md](docs/effects.md)
- **Melee weapons.** Weapon effects go through the same columns; energy blades are a material of the weapon's model,
  so your AC loads a copy of the model with its own material, whose colors the DLL sets.
  [docs/weapon-vfx.md](docs/weapon-vfx.md)
- **HUD.** Each sortie, the DLL hands the game a copy of the HUD movie with every color rewritten for the HUD row.
  [docs/hud.md](docs/hud.md)
- Without the DLL, everything plays stock.

## Build

```
python build.py --game "<...>/ARMORED CORE VI FIRES OF RUBICON/Game" --witchy "<...>/WitchyBND.exe"
```

Needs Python 3, node/npm, cargo (stable) and [WitchyBND](https://github.com/ividyon/WitchyBND) 3.x. It writes
`package/`: the mod folder, `customization_plus.dll` and `customization-plus.me3`.

WitchyBND 3.0.1 refuses to repack 1.09.1 regulations out of the box: raise the version in its
`Assets/Paramdex/AC6/Upgrader/version.txt` to `1_09_1_0042L`.
