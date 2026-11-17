# AC6-Thruster-Customization

Adds a THRUSTER color to the Paint screen in Armored Core VI (1.09.1). Your AC's thruster flames take that color:
boosting, quick boosting, assault boosting, all of it.

## Use

1. Grab the zip from [Releases](../../releases) (or build it, below), extract it anywhere, and launch
   `paint-menu.me3` with [me3](https://github.com/garyttierney/me3).
   It uses its own save, `PaintMenu.sl2`, which me3 copies from your real save on first launch.
2. In the garage, Paint: there's a new THRUSTER row below WEATHERING, on its own panel. It works like DEVICE, one
   color for the whole AC.

The color is stored in the AC's paint data, so it's saved with the AC and carried by designs. Until you pick
one, it's white. Dark colors work too: the flames darken what's behind them instead of glowing.

Below it are two rows for the in-mission HUD:
- **HUD**: its color, and an opacity (its Material menu, 100% down to 0%).
- **HUD WARNING**: the color of warnings. Every state color follows it: red becomes this color, and orange,
  yellow, green and the rest keep their distance from it.

The defaults (the stock pale cyan, red, 100%) are the stock HUD. They apply from the next sortie (AC TEST counts).

It also brings back a setting the game has but hides: how bright the AC's lights are. Confirming the DEVICE row
now opens the same Color / Material panel as the other rows, and its Material is "Light Source Intensity", ten
levels, per part like any material. NPC ACs use it; the stock menu just never let you reach it.

## Known issues

- The Paint screen's pattern thumbnail can stop updating after going in and out of it a few times. That's an me3 bug
  with overridden menu files, not this mod: [me3#902](https://github.com/garyttierney/me3/issues/902).
- The THRUSTER label is in English in every language.

## How it works

- **Paint row.** The row comes from NativePaint (`native_paint.dll` plus its `menu/02_coloring.gfx`, bundled
  unchanged): a small library for extra color rows in the Paint screen. It keeps the color in a part of the
  AC's paint data the game never uses (section 7, color slot 0).
- **Flames.** Every player generator gets a new thruster color type, whose flames are wrapper effects built by
  `fxr/build_fxrs.mjs`. Enemy ACs use the same generators, so when a wrapper spawns, it asks the DLL whether it's on
  your AC. The DLL knows exactly: every character drives its effects through its own slots, and the DLL tags each
  flame with the owner of the slot that spawned it, so an enemy AC's flames stay stock even when it's touching you
  (details and how it was confirmed: [docs/ownership.md](docs/ownership.md)). One pair of flames in the arena intro cutscene comes
  from the cutscene rather than your AC and plays stock. Yours play two recolored copies of the stock flame, one adding the color and one subtracting what the color lacks
  (that's what makes dark colors work). It decides once and sticks to it, since a flame it already started keeps
  playing even if it switches.
- **DEVICE brightness.** Nothing new is added: the game's own material menu already lists the ten light intensity
  materials (90-99) for DEVICE, with their text and live preview, and the value is ordinary paint data. The DEVICE
  row was just built color-only, with no way into that menu. Four hooks build it like the other color rows
  (`dll/src/device.rs`).
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
- **Color.** The DLL gets your color from NativePaint and serves it to the flames as FXR external values, the
  same way as [AC6-FXR-DLL-POC](https://github.com/lugia19/AC6-FXR-DLL-POC). Without the DLL, everything plays stock.
  
  The DLL does all the color math, and the flames only multiply by what it serves. That matters: an FXR can only take
  one external value, run it through a curve and multiply it in, so anything that depends on the whole color (how
  bright it is overall, which decides how much the subtracting copy darkens) can't be done inside the FXR. The DLL
  works out the add, subtract and plain amounts once per color change, and the FXR just reads them.

## Build

```
python build.py --game "<...>/ARMORED CORE VI FIRES OF RUBICON/Game" --witchy "<...>/WitchyBND.exe"
```

Needs Python 3, node/npm, cargo (stable), [WitchyBND](https://github.com/ividyon/WitchyBND) 3.x and a built
NativePaint (`--native-paint <its package folder>`, by default `../native-paint/package`). It writes `package/`: the
mod folder, `paint_menu.dll`, `native_paint.dll` and `paint-menu.me3`.

WitchyBND 3.0.1 refuses to repack 1.09.1 regulations out of the box: raise the version in its
`Assets/Paramdex/AC6/Upgrader/version.txt` to `1_09_1_0042L`.
