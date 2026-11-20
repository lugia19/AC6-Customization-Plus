# Effect columns

How the player's AC, and only it, plays other effects than the game's (`dll/src/columns.rs`, `build.py`). What those
effects are: [tinted-copies.md](tinted-copies.md).

## The game's tables

Some effects depend on the generator: thruster flames come in three kinds (combustion, current, Coral), and so do the
core expansions. The regulation has two tables for that, a row per effect the game spawns and a column per
generator color type (`sfxId<t>`; -1 = the row's own effect, e.g. row 1003010, the assault boost startup flare, one
flare for every type):

- `ThrustersSfxIdParam_PC`: the thruster flames (20 effects), spawned at the thruster animation events.
- `GeneratorTypeSfxIdRplace_PC` (the replace table; no paramdef: 8 s32 columns, rows not sorted): everything else
  that depends on the generator (assault armor, pulse armor...), looked up by `0x1415ce050(type, effect)`.

The type is the generator's thrustersColorType, read through `0x140829de0(chr_module)` (the character at
`[chr_module + 8]`) every time one of these effects is picked: at each thruster event (`0x140786250`, `0x1407859f0`,
`0x1407866c0`, respawn `0x140865510`) and by the expansions' bullet and pulse armor code. Stock uses types 0-2;
columns 3-7 are empty in both tables.

Melee weapons go through the same lookup: every effect a weapon part's animation spawns (TAE event type 96,
`0x140787900`; Elden Ring's `CSChrTaeAnimEvent`) is run, for an AC, through the replace table with the owner's type.
No stock weapon effect has a row, so they all play as is.

## The player's columns

The regulation fills columns 3-6, one per flame type: 3 combustion, 4 current, 5 Coral, 6 ALLMIND.

- **Thruster table:** each flame row gets that type's tinted flame (a bundle; the exhaust fumes, row 410031, stay
  stock).
- **Replace table:** each row gets what the matching stock type uses (ALLMIND: current's), as its tinted copy if it
  has one: so a Coral flame type also gets the Coral expansion effects, recolored.
- **Weapon rows:** the build adds a replace row for each melee weapon effect (`tools/weapons.py` WEAPON_EFFECTS; stock
  in columns 0-2), whose columns 3-6 all play the weapon's tinted bundle: a weapon's look doesn't depend on the flame
  type.

The hook on `0x140829de0` returns a custom column for the player's character and the stock type for everyone else.
Which column: the GENERATOR row's flame type ("Generator's own" = 3 + the generator's stock type). So the same
generator or weapon on an enemy stays stock, and without the DLL nothing ever asks for columns 3-6.

**Bullets.** Some effects are a bullet's, spawned by plain ID (`0x140698380`), without the table: Assault Armor's
blast, the MOONLIGHT / ML-REDSHIFT light waves. The DLL hooks that spawn too and, for the player's bullets (the
owner's handle at bullet + 0xce0) and effects in the custom-column range (990000-999999: the expansions' and weapons'
rows), looks the ID up in the replace table with the player's column.

## Melee weapons' effects

What a melee weapon's animations play: per blade weapon a glow
billboard + light (x00 / x10), a trail (x01 / x11), sparks (x02 / x12) and the slash arcs (x05 / x15), each in one
hue per weapon. Each listed effect gets a tinted copy:

| Weapon | Model | Effects | Notes |
|---|---|---|---|
| HI-32: BU-TT/A pulse blade | 211 | 991000-002, 991005-006, 991010-012, 991015 | 991003-004 have no file |
| Vvc-770LB laser blade | 221 | 991400-402, 991405, 991410-412, 991415 | |
| WLT 101 | 230 | 992100-102, 992105, 992110-111 | its blade is an effect model (88570 / 88571) |
| VP-67LD laser dagger | 241 | 991200-207, 991211-212, 991215 | |
| IA-C01W2: MOONLIGHT | 250 | 991905-907, 991910, 991915-917 | light waves are bullets |
| IA-C01W7: ML-REDSHIFT | 251 | 992205-207, 992210, 992215-217 | light waves are bullets |
| Vvc-774LS laser slicer | 411 | 991500-502, 991505-507, 991510-512, 991515-516 | |
| VE-67LLA laser lance | 610 | 991300-302, 991305, 991307-308, 991310-312, 991315 | 991307 / 991308 are its thrusters (proxies to 411000 / 411010), tinted with the lance; 991309 (its exhaust, built like the AC's 410031) stays stock |
| VP-67EB stun baton | 910 | 991800, 991805-811, 991815-817 | effects only |
| HMMR | 1210 | 991700-703, 991710-715 | effects only |

Left out: the chainsaw and pile bunker (physical sparks), grey smoke effects (991606, 992012). Known gap: some of
MOONLIGHT's effects still play stock (to find with fxr-hunter).
