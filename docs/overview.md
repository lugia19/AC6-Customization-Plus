# How it works

Armored Core VI 1.09.1; every address is for that exe. The mod is a package of game files built from your install
(`build.py`) and a DLL (`dll/`). The features are built from a few mechanisms, each in its own doc:

| Mechanism | What it does | Doc |
|---|---|---|
| Paint UI | the extra rows and their panels, the preview highlight, DEVICE's brightness | [paint-ui.md](paint-ui.md) |
| Storage | where the rows keep their values: free sections of the AC's paint data | [storage.md](storage.md) |
| Effect columns | gives the player's AC its own column in the game's generator-dependent effect tables | [effect-columns.md](effect-columns.md) |
| Tinted copies | effect copies colored by values the DLL serves | [tinted-copies.md](tinted-copies.md) |
| Model copies | neutral effect models and materials for the copies | [model-copies.md](model-copies.md) |
| Material overrides | the player's weapon models' own colors for their shared materials (blades, emissives) | [material-overrides.md](material-overrides.md) |
| HUD movie | the HUD movie rewritten with the HUD row's colors each sortie | [hud.md](hud.md) |

## Features

| Feature | Uses |
|---|---|
| GENERATOR: thruster color, flame type | Paint UI, storage, effect columns (the flame type is the column), tinted copies |
| GENERATOR: expansion color | the same; Assault Armor's blast through the bullet effect hook; neutral model copies |
| GENERATOR: accent color (ALLMIND type) | tinted copies (the fade) |
| HUD: color, warning color, opacity | Paint UI, storage, HUD movie |
| DEVICE's brightness | Paint UI only: the game's own materials and menus |
| WEAPON VFX: effects | Paint UI, storage, effect columns, tinted copies (light waves through the bullet hook) |
| WEAPON VFX: hit effects | the hit hooks (effect-columns.md, "Melee hit effects"), tinted copies |
| WEAPON VFX: blades, glows | Paint UI, storage, material overrides |

Without the DLL, everything plays stock: nothing ever asks for the custom columns, no model gets entries for the
weapon materials' keys, nobody's hits get the copies, the HUD movie is used as shipped (it looks stock), and the rows
aren't there.

## The DLL

`dll/src`, by mechanism: `rows.rs`, `panel.rs`, `ui.rs` (Paint UI); `ext_values.rs` (served values), `columns.rs`
(effect columns); `generator.rs`, `hud.rs` / `hud_movie.rs`, `weapons.rs`, `hits.rs` (features); `game.rs` (memory helpers,
the player and paint data pointer chains). One thread (`lib.rs` update loop) applies changed settings every 200 ms.
Log: `Game\debug-logs\customization_plus.log`.

Hooks (17). The game's Arxan protection puts back code it guards at irregular times, removing hooks, so the me3
profile turns it off (`disable_arxan`).

| Hook | Function | For |
|---|---|---|
| `0x140f418d0` | Paint row builder | keeps the edit context while it runs (rows.rs) |
| `0x140d3cb00` | list description hand-off | appends our rows (rows.rs) |
| `0x140f43610` | row kind -> panel | our rows' and DEVICE's panels (rows.rs) |
| `0x14045de80` | is-normal-slot | DEVICE built as a color row (rows.rs) |
| `0x140f3fd90` | Paint panels setup | hides our panel and list host; keeps the panel state (panel.rs) |
| `0x140f35c40` | material text | "Brightness N%" (panel.rs) |
| `0x14102c4c0` | preview highlight | WEAPON VFX flashes the weapons (weapons.rs) |
| `0x142b56480` | FXR external value read | serves the colors (ext_values.rs) |
| `0x140829de0` | a character's generator color type | the player's column (columns.rs) |
| `0x140698380` | bullet effect spawn | the player's bullets' effects through the columns (columns.rs) |
| `0x1408f0190` | muzzle flash spawn | the player's shots' muzzle flashes through the columns (columns.rs) |
| `0x1410ff770` | a part model's paint | the player's weapon models' color entries (weapons.rs) |
| `0x140995870`, `0x140995780` | hit effect dispatchers | note the player's melee hits (hits.rs) |
| `0x140b9f390` | hit effect list | swaps in the weapon's hit copies (hits.rs) |
| `0x141609040` | menu movie lookup | the HUD movie each sortie (hud.rs) |
| `0x1415f40b0` | Scaleform file opener | serves the rewritten HUD movie (hud.rs) |

## The build

`build.py` (pieces in `tools/`, `fxr/build_fxrs.mjs`) writes `package/`:

| File | Made from | What changes |
|---|---|---|
| `regulation.bin` | the game's | custom columns 3-6 of the two effect tables; replace rows for the weapons' effects (`tools/params.py`) |
| `sfx/sfxbnd_commoneffects.ffxbnd.dcx` | the game's (and ALLMIND's effects from `sfxbnd_m30`) | tinted copies and bundles; neutral model copies (`fxr/build_fxrs.mjs`, `tools/models.py`) |
| `material/allmaterial.matbinbnd.dcx` | the game's | neutral materials for those models (`tools/models.py`) |
| `shader_d3d12/shaderbdle.shaderbdlebnd.dcx` | the game's | keys on the weapon materials' color params (`tools/shaders.py`; cached in `cache/`, slow to repack) |
| `menu/02_coloring.gfx` | the Paint movie | the extra rows' clips, their panel, the tall list (`tools/build_gfx.py`) |
| `menu/01_000_fe.gfx` | the HUD movie | neutral transforms, a table of its colors (`tools/prepare_hud.py`) |
| `customization_plus.dll`, `customization-plus.me3` | `dll/`, the repo | |

The DLL and the build share some tables: the build checks the flame type labels agree, and writes the weapon table
(`dll/src/weapon_table.rs`, committed so the DLL builds on its own) from `tools/weapons.py` and the game's data.

## Adding a melee weapon

One entry in `WEAPONS` (`tools/weapons.py`): its EquipParamWeapon ID, the default color of its effects, its part's
colored materials (blades, emissives: a material whose colors are the weapon's look), and effects to leave stock. The
build reads the rest from the game's data: its model and behavior variation, the effects its animations and bullets
play, its hit effects, its materials' color parameters; and it writes the DLL's table. The build then prints what it
found per weapon.

Limits (a modded weapon may hit them):
- **Twenty weapons.** Their colors are saved in two free paint sections' ten color fields each
  ([storage.md](storage.md)), in `WEAPONS`' order (never reorder it). More need another place to save their colors
  (section 7 still has a color and its pattern colors free).
- **Effect IDs 990000-999999.** The bullet and muzzle flash hooks only remap that range (`CUSTOM_COLUMN_EFFECTS` in
  `columns.rs` and `tools/weapons.py`); the build stops on others.
- **Float5 colors.** A colored material's params must be Float5 (rgb, alpha, intensity; the build stops on Float4),
  and a param can be in only one of a weapon's materials (its entries are for all of the model's materials).
- **Files in the game folder.** The build reads parts, effects and params from the game folder; a weapon whose files
  come from another mod's package needs them there (or the build pointed at them).
- **Hit effects** on terrain and AC bodies only (concepts 10, 103), as for the stock weapons.
