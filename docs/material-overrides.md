# Material overrides

How the player's melee weapons' materials (blades, emissives) get their colors (`tools/weapons.py`,
`tools/shaders.py`, `dll/src/weapons.rs`). The class names below are Elden Ring's (same engine, which keeps them).

## The colored materials

| Weapon | Material (shader) | What it colors | Colors |
|---|---|---|---|
| the energy blades (211, 221, 241, 250, 251, 411, 610) | `P[WP_xxx0]_Laser` (`P[LaserBlade]_001`) | the blade | `color_6`, `_8`, `_10`, `_12`, `_13` |
| WLT 101 (230) | `P[WP_0230]_LB_Emissive_Glow` (`P[Laser]_Emissive_Glow`) | the red glow (its blade is an effect model, [model-copies.md](model-copies.md)) | `color_0`, `_2`, `_4`, `_6` |
| HMMR (1210) | `P[WP_1210]_Plasmawire` (`P[ACParts][Plasma_spinner]_Emissive_Glow`) | the chain | `color_0`, `_3` |

All Float5 (rgb, alpha, intensity), read from the matbins (`tools/weapons.py` material_colors: every param named
color). Those at intensity 0 add no light and stay stock (some blades list `color_15`, white at 0; overriding it
erases the blade).

What the blade's colors draw (marker test on ML-REDSHIFT, normal and charged swings): `color_12` the main body,
`color_6` bright core streaks, `color_8` sparse filaments at the tip and end, `color_10` the pale wispy highlight
strands; `color_13` nothing seen. With all of them black the blade is gone: all its light is these.

## Per-model overrides

Materials are shared: one object per name (`CS::MtdRepository`), drawn by every model that uses it, so writing its
params recolors every AC. What's per model is its **material param modifier** (`GXBS::GXMaterialParamModifier`),
which is how paint colors shared AC part materials per AC:

- Every part model (one FLVER of one parts slot of one AC) has one: part model `+0x10` its model item
  (`CS::CSFD4ModelItem`; its attach system at `+0x18`), item `+0x50` the modifier component
  (`CSFD4ModelItemMaterialParamModifier`), component `+8` the modifier. Its entries, `[modifier + 8]` ->
  `{allocator, begin, end, capacity}`, 0x40 each: +0 five values, +0x24 the blend, +0x28 a material name, +0x30 its
  hash, +0x34 the **param key**, +0x38 a material ID, +0x3c "all materials".
- A material param has a key (`param + 0x1c`): the CRC-32 of a name's UTF-16LE bytes (`0x14263c0a0`), copied from its
  shader's `.metaparam` when the material loads. Paint's: the body's `color_10` is keyed `Color_Main`, and paint adds
  `Color_Main` entries for material ID 60 (`g_GX_MaterialID`, the weapon bodies').
- Drawing a model with entries (`0x1422ffaa0`), every param looks up an entry with its key: for its material's ID, or
  its name when the ID is 0, else one for all materials; a match replaces its value (Float5 `0x14268a8a0`:
  `rgb = intensity * entry rgb`, both linearized when the param is sRGB, blended by the entry's blend). A model without
  entries draws the material as is.

The blade and emissive shaders give their colors no key, so nothing reaches them.

## What the mod does

**Keys** (build, `tools/shaders.py`): the shipped `shader_d3d12/shaderbdle.shaderbdlebnd.dcx` gives each colored
param the key of `CP_<param name>` (e.g. `CP_P_LaserBlade__001_snp_0_color_12`), in its shader's `.metaparam` (a
param record: its name, and the key at `+0x30`). Every material of those shaders now listens to entries no model has,
so all draw stock. The bundle is 218 MB and slow to repack: cached in `cache/` until the game's file or the keys
change. The ray-tracing bundle (`shaderbdle_[rt]`) has only RT programs and an `.rtmetaparam` for these shaders: not
patched (ray-traced reflections may show the stock colors). HMMR's `color_0` had a key, `Parts_Emissive_Color`
(MaterialEx row 1110, which no param uses); ours replaces it.

**Entries** (DLL): on the player's weapon models only. The AC model object `[ChrIns + 0x47f8]` has a model list per
parts slot at `+0xa8 + slot * 8` (`{+8 up to 16 part models, +0x88 the count}`; slot 6 the left arm's weapon, 7 the
left back's). The game paints each model with `0x1410ff770(part model, paint section, section data)` when it's built
(a part swap, an AC TEST or sortie load) or repainted; a hook after it, when the section is a weapon unit's (8 left
arm, 13 left back) and the model is in the player's list, adds an entry per color with the modifier's setter
`0x1423ebdc0(modifier, 0 (all materials), L"CP_...", values[5], blend 1)` (it hashes the name, and updates an entry
with the same key). Adding has to happen there, on the game's thread: the entries are a vector the game grows (paint,
the respawn line, status effects) and the renderer reads without a lock. Entries live as long as the model: only its
destruction clears them.

**Values**: each color takes the chosen color, as white as the stock one is (its saturation) and at its stock
brightness (brightest channel), stock alpha and intensity. The pale layers have to stay pale: a pure green on every
color makes the blade invisible. Black thus keeps a blade's pale highlight grey, and HMMR's half-saturated chain
whitish. A weapon without a color gets its stock values. A color picked in the Paint screen reaches the models when
they're next built: the weapons only show in AC TEST and sorties, which build them.

Which weapons the player has: the player's assembly `[ChrIns + 0x47e0]`, weapons at `+0xF4 + slot * 4` (EquipParamWeapon
IDs; slot 0 left arm, 1 right arm, 2 left back, 3 right back; the game's getter `0x1406f5890`). Melee weapons only go
in the left slots. The Paint screen's preview draws no emissives: not handled.
