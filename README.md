# AC6-Customization-Plus

More to customize in Armored Core VI's Paint screen (1.09.1), saved with each AC:

- **Thruster color and flame type.** Your AC's thruster flames take the color: boosting, quick boosting, assault
  boosting, all of it. The type picks the flame itself, whatever generator you have.
- **HUD color, warning color and opacity** for the in-mission HUD.
- **Light brightness**, a setting the game has but hides.

## Use

1. Grab the zip from [Releases](../../releases) (or build it, below), extract it anywhere, and launch
   `customization-plus.me3` with [me3](https://github.com/garyttierney/me3).
   It uses its own save, `CustomizationPlus.sl2`, which me3 copies from your real save on first launch.
2. In the garage, Paint: below WEATHERING there's a new panel, ENTIRE AC, with two rows. Each is one setting for
   the whole AC, whichever part tab is open. Confirming a row opens its panel, like MAIN's Color / Material.

**THRUSTER**: a color, a flame type and an accent color. Until you pick a color, it's white. Dark colors work
too: the flames darken what's behind them instead of glowing. The accent color isn't used yet. The types:
- **Generator's own** (the default): the flames of the generator you have equipped.
- **Combustion**, **Current**, **Coral**: the game's three kinds of flame. They differ in shape and detail, not only
  in color: Current has its own flame model, Coral adds sparkles and haze.
- **ALLMIND**: the Current flame with the extras of ALLMIND's thrusters: a spray of sparks, a steady light and a
  flash when a thruster starts.

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

Coming from 0.1.0 (AC6-Thruster-Customization)? The save file has a new name: rename `PaintMenu.sl2` to
`CustomizationPlus.sl2` in your save folder to keep what you did there.

## Known issues

- The Paint screen's pattern thumbnails can break after going in and out of it, or after a mission: the PATTERN
  thumbnail stops updating, or some parts' swatches show no pattern. The AC itself keeps it. That's an me3 bug with
  overridden menu files, not this mod (it happens without the DLL too):
  [me3#902](https://github.com/garyttierney/me3/issues/902).
- The new rows' labels and choices are in English in every language.

## How it works

- **Paint rows.** The rows are list items the DLL adds to the list the game's row builder made, built the way the
  game builds its own color rows (`dll/src/rows.rs`), shown by clips added to the Paint movie (`tools/build_gfx.py`
  patches `menu/02_coloring.gfx`). Their panels are built by the DLL from the game's own menu pieces (`dll/src/panel.rs`):
  the game's color picker, and option lists like the pattern's Size. A row keeps its colors and choice in a part of
  the AC's paint data the game never uses, which is why a row is one value for the whole AC and not per part.
  Details: [docs/paint-rows.md](docs/paint-rows.md).
- **Flames.** Every player generator gets a new thruster color type, whose flames are wrapper effects built by
  `fxr/build_fxrs.mjs`. Enemy ACs use the same generators, so when a wrapper spawns, it asks the DLL whether it's on
  your AC. The DLL knows exactly: every character drives its effects through its own slots, and the DLL tags each
  flame with the owner of the slot that spawned it, so an enemy AC's flames stay stock even when it's touching you
  (details and how it was confirmed: [docs/ownership.md](docs/ownership.md)). One pair of flames in the arena intro cutscene comes
  from the cutscene rather than your AC and plays stock. Yours play two recolored copies of the stock flame, one adding the color and one subtracting what the color lacks
  (that's what makes dark colors work). It decides once and sticks to it, since a flame it already started keeps
  playing even if it switches.
- **Flame types.** The game keeps a flame per thruster effect and generator type (combustion, current, Coral): 20
  effects, three flames each. Every wrapper knows its effect's three flames, and on your AC plays the recolored
  copies of the one for the type you chose (served to the effects as another external value). ALLMIND is a boss,
  not an AC, with thruster effects of its own; its type plays the current flame plus a copy of the boss's effect
  with the flames taken out.
- **DEVICE brightness.** Nothing new is added: the game's own material menu already lists the ten light intensity
  materials (90-99) for DEVICE, with their text and live preview, and the value is ordinary paint data. The DEVICE
  row was just built color-only, with no way into that menu. The row hooks build it like the other color rows,
  and its panel leads to that menu (`dll/src/panel.rs`).
- **HUD.** The game loads the HUD movie (`menu/01_000_fe.gfx`) once and builds each mission's HUD from it. The
  package ships a prepared copy of it. Every time a sortie asks for the movie, the DLL reads the HUD rows,
  drops the loaded movie and hands the game's loader a copy of the file it opened at boot, rewritten for them
  (`dll/src/hud.rs`). Nothing is written to disk and nothing is remembered between sorties.

  The movie holds about 2,400 colors: shape fills and outlines, text colors and color transforms. The build
  (`tools/prepare_hud.py`) finds them all and appends a table of where they are to the movie, past the length the
  game reads; it also gives each of the movie's 54 visible top-level elements a neutral color transform. The DLL
  (`dll/src/hud_movie.rs`) then only runs each color in the table through one rule and writes the opacity into
  those transforms' alpha. The rule: the movie's colors fall in two groups. Pale and grey ones are the HUD's base
  color (`#DFFCFF` and its shades): they become the HUD color at the same brightness. Saturated ones are state
  colors (the stagger bar's orange and red, reload and overheat, low AP, boss HP green, condition gauge
  purple...): their hue is rotated, and their saturation and brightness scaled, by what takes pure red to the
  warning color.
- **Color.** The DLL reads your color from the THRUSTER row and serves it to the flames as FXR external values, the
  same way as [AC6-FXR-DLL-POC](https://github.com/lugia19/AC6-FXR-DLL-POC). Without the DLL, everything plays stock.
  
  The DLL does all the color math, and the flames only multiply by what it serves. That matters: an FXR can only take
  one external value, run it through a curve and multiply it in, so anything that depends on the whole color (how
  bright it is overall, which decides how much the subtracting copy darkens) can't be done inside the FXR. The DLL
  works out the add, subtract and plain amounts once per color change, and the FXR just reads them.

## Build

```
python build.py --game "<...>/ARMORED CORE VI FIRES OF RUBICON/Game" --witchy "<...>/WitchyBND.exe"
```

Needs Python 3, node/npm, cargo (stable) and [WitchyBND](https://github.com/ividyon/WitchyBND) 3.x. It writes
`package/`: the mod folder, `customization_plus.dll` and `customization-plus.me3`.

WitchyBND 3.0.1 refuses to repack 1.09.1 regulations out of the box: raise the version in its
`Assets/Paramdex/AC6/Upgrader/version.txt` to `1_09_1_0042L`.
