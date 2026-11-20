# Model copies

Two cases where a tint isn't enough because the color is in a model's material (`tools/models.py`, `tools/weapons.py`,
`dll/src/weapons.rs`). The class names below are Elden Ring's (same engine, which keeps them).

## Neutral effect models

Some effect models (RichModels) get their color from their material: light blue, or orange for 83038. A tint can only
multiply that. The copies ([tinted-copies.md](tinted-copies.md)) use neutral versions instead:

- the expansions' 83038, 88231, 88232, 88235;
- the MOONLIGHT / ML-REDSHIFT light waves' 83040, 83042, 83140, 83142;
- WLT 101's blade, 88570 / 88571 (its blade is an effect model, not part of the weapon).

Each is copied as model + 10000 (s8xxxx -> s9xxxx), its material renamed the same way (same length, so the model's
reference is patched in place), and that material copied into `allmaterial.matbinbnd.dcx` with its colors (and
`g_GlowScale`) greyed to their luminance, like the copies' own colors. The copies' resource lists point at them.

## The player's blade weapons

An energy blade's body is a material of the weapon's model (`P[WP_xxx0]_Laser`, shader `P[LaserBlade]_001`), with
fixed colors (`color_6`, `_8`, `_10`). Materials are shared by every model that uses them, and changing one recolors
that blade on every AC: global. So the player's AC loads its own copy of the part, with its own material.

**The copies** (build): for each weapon whose blade is a material (`WEAPON_BLADE_MATERIALS`: models 211, 221, 241,
250, 251, 411, 610), the part (`wp_l_0221.partsbnd.dcx` and its `_l` LOD) is copied as model 9xxx (`wp_l_9221`),
stock except its file name and its blade material reference (`P[WP_9220]_Laser`). Its textures are copied too
(`wp_9221.tpf.dcx` + `_l`): the game loads them by number, and without them the body renders unpainted. The material
is copied into `allmaterial`.

**Loading them** (DLL): the AC parts loader `0x1410d8460(parts, assembly, ...)` builds each slot's file name with
`0x1415d0080(wstring *out, param row, ...)`: prefix + `%04d` of `equipModelId`, as an FD4 wide string (+8 the
characters, inline up to 7, else a pointer there; +0x18 the length, +0x20 the capacity). A hook on the loader notes,
in a thread-local, whether it's loading the player's parts object (`[player ChrIns + 0x47e8]`); the name builder runs
inside it, and its hook then turns `WP_L_0221` into `WP_L_9221` (and `_L`). Other ACs and the garage's assembly
preview load the stock part.

**Their colors** (DLL, `weapons.rs` Weapons): the material repository `[0x144e1cc38]` (`CS::MtdRepository`),
`0x141344fe0(repository, L"P[WP_9220]_Laser")` -> `MtdResCap`, the material at `+0x78`;
`0x142300730(material, L"..._color_6")` -> the parameter (`GXMaterialParam_HDRColor`), its rgba at `+0x40`, intensity
at `+0x50`; then `0x1423020a0(material)` rebuilds the material's constant buffer, as the game's own matbin reload
does. Each blade color takes the chosen color at its stock brightness (brightest channel); intensities stay. Set
when the weapon's color changes; a weapon without one keeps its stock blade.

Which weapons the player has: the player's assembly `[ChrIns + 0x47e0]`, weapons at `+0xF4 + slot * 4` (EquipParamWeapon
IDs; slot 0 left arm, 1 right arm, 2 left back, 3 right back; the game's getter `0x1406f5890`). Melee weapons only go
in the left slots.
