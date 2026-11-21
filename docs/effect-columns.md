# Effect columns

How the player's AC, and only it, plays other effects than the game's (`dll/src/columns.rs`, `build.py`). What those
effects are: [tinted-copies.md](tinted-copies.md).

## The game's tables

Some effects depend on the generator: thruster flames come in three kinds (combustion, current, Coral), and so do the
core expansions. The regulation has two tables for that, a row per effect the game spawns and a column per
generator color type (`sfxId<t>`; -1 = the row's own effect, e.g. row 1003010, the assault boost startup flare, one
flare for every type):

- `ThrustersSfxIdParam_PC` (param 0xB4): the thruster flames (20 effects), spawned at the thruster animation events.
- `GeneratorTypeSfxIdRplace_PC` (param 0xB5, the replace table; no paramdef: 8 s32 columns, rows not sorted):
  everything else that depends on the generator (assault armor, pulse armor...).

Each is read only through its lookup, `int lookup(type, row)`: `0x1415cdd90` (thrusters) and `0x1415ce050`
(replace). A missing row, a -1 column or a type outside 0-7 gives back the row itself. Checked statically (every
caller, every getter of the two params) and at runtime: over a session, the generic param getter `0x1415c8980` was
called for these two params exactly as often as their lookups ran.

The type is the generator's thrustersColorType, read through `0x140829de0(chr_module)` (the character at
`[chr_module + 8]`) every time one of these effects is picked: at each thruster event (`0x140786250`, `0x1407859f0`,
`0x1407866c0`, respawn `0x140865510`) and by the expansions' bullet and pulse armor code. Stock uses types 0-2;
columns 3-7 are empty in both tables.

Melee weapons go through the same lookup: every effect a weapon part's animation spawns (TAE event type 96,
`0x140787900`; Elden Ring's `CSChrTaeAnimEvent`) is run, for an AC, through the replace table with the owner's type.
No stock weapon effect has a row, so they all play as is.

## The player's columns

Columns 3-6, one per flame type: 3 combustion, 4 current, 5 Coral, 6 ALLMIND. They aren't in the regulation, which
stays stock (other mods ship their own): the build writes them to `dll/src/effect_table.rs`, and hooks on both
lookups answer types 3-6 from it, leaving the rest to the game.

- **Thruster table:** each flame row gets that type's tinted flame (a bundle; the exhaust fumes, row 410031, stay
  stock).
- **Replace table:** each row gets what the matching stock type uses (ALLMIND: current's), as its tinted copy if it
  has one: so a Coral flame type also gets the Coral expansion effects, recolored.
- **Weapon rows:** a replace row for each melee weapon effect (below; the game has none, so stock types get the
  effect itself), whose columns 3-6 all play the weapon's tinted bundle: a weapon's look doesn't depend on the flame
  type.

The hook on `0x140829de0` returns a custom column for the player's character and the stock type for everyone else.
Which column: the THRUSTERS row's flame type ("Generator's own" = 3 + the generator's stock type). So the same
generator or weapon on an enemy stays stock, and without the DLL nothing ever asks for columns 3-6.

**Bullets.** Some effects are a bullet's, spawned by plain ID (`0x140698380`), without the table: Assault Armor's
blast, the MOONLIGHT / ML-REDSHIFT light waves. The DLL hooks that spawn too and, for the player's bullets (the
owner's handle at bullet + 0xce0) and effects in the custom-column range (990000-999999: the expansions' and weapons'
rows), looks the ID up in the replace table with the player's column.

**Muzzle flashes.** A bullet's `muzzleFlashSfxId` / `_2` (MOONLIGHT's 991905 / 991915, ML-REDSHIFT's 992205 / 992215)
are spawned on the shooter, by `0x1408f0190(fire_module, info)` (the shooter's ChrIns at fire_module + 8; the IDs at
info + 8 / + 0x10), for melee-launched bullets and gun fire alike. The DLL hooks it too: for the player's shots, both
IDs go through the replace table the same way, in a copy of `info` (gun fire reuses it for each dummy poly).

## Melee weapons' effects

The build reads them from the game's data (`tools/weapons.py` derive), for each weapon in its `WEAPONS` list:

- **Animations:** every TAE event 96 in the weapon part's animations (`wp_l_<model>.partsbnd.dcx`, its anibnd).
- **Bullets:** the effect fields (`sfxId_Bullet`, `sfxId_Hit`, `muzzleFlashSfxId`...) of every bullet its behaviors
  fire (BehaviorParam_PC rows of its behaviorVariationId), and of the bullets those fire.
- Effects without a file are skipped; a weapon's `stock` list leaves effects stock on purpose; an effect outside
  990000-999999 (what the DLL can give columns) stops the build.

Per blade weapon that's a glow billboard + light (x00 / x10), a trail (x01 / x11), sparks (x02 / x12) and the slash
arcs (x05 / x15), each in one hue per weapon. As of 1.09.1:

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
| VP-67EB stun baton | 910 | 991800-802, 991805-812, 991815-817 | effects only (991801 / 802 / 812 draw nothing) |
| 44-143 HMMR | 1210 | 991710-715 | effects only (991700-703 are the explosive thrower's) |

Left out: the chainsaw and pile bunker (physical sparks), grey smoke effects (991606, 992012).

## Melee hit effects

A hit's effects (sparks on a target or the ground) come from `HitEffectSfxParam`, not from these tables: a row per
surface concept, a column per attack type and size, so every laser weapon's slash makes the same 250006, enemies'
included. All of them are picked by one function, `0x140b9f390(out, material, type, size, row offset, concept1,
concept2, mod100)` (an FD4 vector of `{i32 id; u8 follow}`), called only under the hit dispatchers `0x140995870(dmg)` /
`0x140995780(list, dmg)` (on the damage manager's thread or its job workers) for weapon hits.

`dll/src/hits.rs` hooks the three: a dispatcher keeps the hit's weapon in a thread-local when the player made it, and
the list builder then swaps each effect that weapon has a copy of for the copy's bundle (stock + model × 100,000 +
60,000; the pair at + 0 / + 30,000).

- **The player:** `0x140b3c790(attacker)` (dmg + 0x1e8) gives the character behind a character or bullet, through
  the bullets that fired it; compared with the player's.
- **The weapon:** dmg + 0x64, the BehaviorParam ID that launched the attack, is 100,000,000 + variation × 1000 + n for
  the player's; the stun baton's bullets have none, so their bullet ID (dmg + 0x150) / 1000. The variation is
  EquipParamWeapon behaviorVariationId (melee weapons 20000-20999). Ranged weapons and anyone else's hits stay stock.
- **The effects** (derived by the build: the weapon's attacks and bullets, by material, type and size, on the terrain
  and AC body concepts 10 and 103; physical (Iron) hits are metal sparks and stay stock): 250006 laser weapons, 260006 pulse blade, 265006/265007
  WLT 101 and ML-REDSHIFT, 273006/273007 MOONLIGHT (and the slicer's and dagger's Energy_Green attacks), 255000/255005/
  255006 HMMR, 257006 stun baton. Left stock: criticals (they come from the target, by another path), terrain debris,
  shield and Pulse Armor ripples (the target's color), and the HMMR's scorch decal (decals are their own system).
