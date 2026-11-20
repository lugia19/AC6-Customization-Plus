# Tinted copies

How the effects the player's columns play ([effect-columns.md](effect-columns.md)) take the player's colors:
copies of the stock effects built by `fxr/build_fxrs.mjs`, colored by FXR external values the DLL serves
(`dll/src/ext_values.rs`), the way [AC6-FXR-DLL-POC](https://github.com/lugia19/AC6-FXR-DLL-POC) does.

## The copies

A copy is the stock FXR (and every FXR it references, remapped to their copies) with every color whitened to its
brightness and multiplied by external values. An FXR can only take an external value, run it through a curve and
multiply it in, so the copies do no color math of their own: the DLL works out what each kind of copy multiplies by,
once per color change.

- **Flames** (the thruster color): two copies, played together.
  - add: the flame with the color added (lights and lens flares too); alpha-blended particles (smoke) get the plain
    color, which they can show as is;
  - remove: the additive particles again, subtracting what the color lacks from the background, so dark colors
    darken it (additive flames can't get darker than what's behind them). Nothing for bright colors.
- **Expansions** (the expansion color): the same pair, greyed by luminance so white is about as bright as stock.
- **Melee weapons** (the weapon's color): the same pair.

A column holds one effect ID, so the pairs are played through a **bundle**: an FXR holding only proxies to the
copies, ending when the game ends the stock one (external value 0, the spawner's termination signal).

Every new FXR needs its source's resource list (its textures and models); the build copies them, with the neutral
models swapped in ([model-copies.md](model-copies.md)).

### IDs

| Offset from the stock ID | What |
|---|---|
| + 9,000,000 | bundle (a flame row's per stock type; an expansion effect's) |
| + 9,100,000 / + 9,200,000 | add / remove copies (flames, expansions) |
| + 8,000,000 | ALLMIND bundle (from the row's current flame) |
| + 9,300,000 / + 9,400,000 | ALLMIND: accent copies (add / remove) |
| + 9,500,000 / + 9,600,000 | ALLMIND: thruster color copies (add / remove) |
| + 9,700,000 / + 9,800,000 | melee weapon copies (add / remove) |
| + 9,900,000 | melee weapon bundle |
| + model × 100,000 + 0 / + 30,000 / + 60,000 | a melee weapon's hit effect: add, remove, bundle |

The build stops on any collision with an existing effect or another copy.

## The served values

`0x142b56480`, effectively `float *get(value_block, float *out, int id)`, is how every effect reads an external
value; the hook answers the mod's IDs itself, and nothing else provides them. A color is a block of nine IDs:

| Channels | Value (per red, green, blue; c = 0-1) |
|---|---|
| 0-2, add | c for bright colors; for dark ones its linear intensity, since the flames multiply it into HDR colors up to ~50 (#050505 at 0.02 would still leave those above 1: white) |
| 3-5, remove | what the color lacks (1 - c), for dark colors only |
| 6-8, plain | c |

Each is served as 1 + the value; 0 means no DLL. "Bright" goes smoothly from 0 at a brightest channel of 64/255 to 1
at 160/255.

| Block | Color |
|---|---|
| 900001-900009 | thruster color |
| 900012-900020 | accent color (ALLMIND) |
| 900021-900029 | expansion color |
| 900000 + 10 × model (9 IDs) | a melee weapon's color (e.g. 902210 for the Vvc-770LB, model 221) |

A weapon without a color gets its default, close to its stock hue (`weapons.rs` WEAPONS).

## Flame types

- **Combustion, Current, Coral** (columns 3-5): that type's stock flames, tinted. They differ in shape and detail,
  not only in color: Current has its own flame model, Coral adds sparkles and haze.
- **ALLMIND** (column 6): ALLMIND is a boss, not an AC, with thruster effects of its own (in `sfxbnd_m30`). Its type
  plays, per flame row, the current flame or the boss's effects, scaled down for an AC:
  - regular boosting: the current flame plus the boss's 642001 (spark spray, steady light, start flash; its own
    flames left out);
  - quick boost: the boss's 642002 at 40%;
  - held assault boost: the boss's 642142 at 30%, made continuous (a burst from a special move);
  - other rows: the current flame.

  Every one fades from the accent color to the thruster color. A particle can only multiply in served values, so it
  can't blend two of them: each effect is drawn twice, an accent copy and a thruster color copy, weighted so the two
  (additive) add up to the fade, by particle age (0.15 s) or, for 642142, by its own mint-to-cyan hue. The copies share
  their random seeds, so they line up.
