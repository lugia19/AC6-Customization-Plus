# Storage

Where the extra rows keep their values (`dll/src/rows.rs`): in parts of the AC's paint data the game never uses, so
they're saved with the AC and carried by designs, with nothing of our own to save.

## The paint data

14 sections of 0x44 bytes, saved and carried by designs as one 0x3b8 block:

| Offset | Field |
|---|---|
| +0x00 | header (0xff) |
| +0x04 | weathering |
| +0x08 | 6 colors (RGBA bytes) |
| +0x20 | 6 materials |
| +0x2c | pattern design, size |
| +0x30 | 4 pattern colors |

Sections: 0 head, 1 core, 2 R arm, 3 L arm, 4 legs, 5 R weapon, 8 L weapon, 11 the leftover Extension unit, 12 R
back, 13 L back. The part categories (`0x14045d960`) never write 6, 7, 9 and 10; 10's header gets written by something
else and Sundance's Extension parts mod uses it.

The paint writers: `0x14045afc0` color (+8), `0x14045b070` material (+0x20 + slot * 2; reached from `0x140e0d100`),
`0x14045aee0` pattern design (+0x2c), `0x14045af90` pattern size (+0x2d), `0x14045af30` pattern color (+0x30),
`0x14045b020` weathering (+4).

The player's paint block (`dll/src/game.rs` paint_value): `[[[GameDataMan]+8]+0x788]+0x14C` (GameDataMan's pointer is
at `0x144e03a08`; the chain is null while loading). The Paint screen edits a working copy in its edit context
(+0x40), which the panels write into ([paint-ui.md](paint-ui.md)).

## What the rows use

| Section | colors 0-5 | materials 0-5 | pattern colors 0-3 |
|---|---|---|---|
| 7 | THRUSTERS: 0 thruster color, 1 accent color; WEAPON VFX: 2 expansion color; HUD: 3 HUD color, 4 warning color; 5 free | 0 THRUSTERS' flame type, 1 HUD's opacity; 2-5 free | free |
| 9 | WEAPON VFX: weapons 0-5 | free | weapons 6-9 |
| 6 | WEAPON VFX: weapons 10-15 | free | weapons 16-19 |
| 10 | not ours (written by something else; Sundance's Extension parts mod uses it) | | |

Each row says where its values are (`rows.rs` STORAGE: a section, its first color, its choice's material); THRUSTERS is
on section 7 because version 0.1.0 stored its thruster color there. Unused in all of them: weathering, the pattern
design and size.

WEAPON VFX keeps one color per melee weapon, in `WEAPONS`' order (`tools/weapons.py`): section 9's ten color fields
(its colors, then its pattern colors), then section 6's, twenty at most. Its panel shows the two equipped ones
(`rows::claim_dynamic`): the weapon in a slot decides which field the entry edits, so each weapon keeps its color when
swapped out and back. Its third entry, the expansion color, is one for every expansion, in section 7's third color
(where the GENERATOR row, since split, kept it).

## Encoding

The game writes alpha 0xFF into every color it stores (unused sections hold its default grey; old saves can have
black with alpha 0). A color picked in an extra row is stored with alpha 0xFE; anything else means "not picked" and
the row shows its default. A choice is stored as 0xFE00 + its index, not as the material's ID (the unused sections
hold the default material, 18); anything else, or an index past the row's choices, means the default.

## Why it's safe

Checked (2026-11-20): every paint writer (color, material, pattern design / size / colors, weathering) takes its
sections from the category table `0x14045d960`, whose lists are {0, 1, 2, 3, 4} (ENTIRE FRAME), {5, 8, 12, 13, 11}
(ALL UNITS) and one section per single part: never 6, 7, 9 or 10. The pattern color writer `0x14045af30` writes
`+0x30 + (section * 0x11 + index) * 4` for listed sections only. Saves and designs move the block as a whole
(`0x14045ad80` / `0x14045ae80` stream all 0x3b8 bytes). At runtime, test values in section 9's pattern colors
survived the Paint screen's save, a COLOR SET preset, pattern changes on ENTIRE FRAME and ALL UNITS, a design save /
load (another design replaced them, ours restored them) and a restart.

## Why a row is for the whole AC

A stock row's color lives in the selected part's own sections (slot k of head, core...), which is what makes it per
part. An extra row has no slot in those sections (they only have six colors), so its colors live in a section of
its own and the selected part is ignored. Per-part values would need a place per part: ten sections' worth, and
only three are free (or one section's ten color fields for a single per-part color).
