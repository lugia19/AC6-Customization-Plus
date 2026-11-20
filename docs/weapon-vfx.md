# Melee weapon colors (WEAPON VFX)

How the melee weapons get the player's colors (Armored Core VI 1.09.1). Research notes, test tools and the Elden Ring
cross-checks are in the AC6_tools workspace (`Research\weapon-vfx`).

## What a melee weapon draws

- **Effects** (glow, trail, sparks, slash arcs) come from the weapon part's own animations (`parts/wp_l_<model>`,
  its TAE), event type 96: spawn an effect at a dummy poly. Every type-96 spawn goes through `0x140787900`, which
  for an AC runs the ID through `GeneratorTypeSfxIdRplace_PC` with the owner's generator color type (`0x140829de0`,
  the type the flames hook overrides for the player). Light waves (MOONLIGHT, ML-REDSHIFT) are bullets; the bullet
  effect hook runs the player's 99xxxx bullet effects through the same table.
- **Energy blades** are a material of the weapon's model (`P[WP_xxx0]_Laser`, shader `P[LaserBlade]_001`), with
  fixed colors (`color_6`, `_8`, `_10`). Materials are shared by every model that uses them. WLT 101's blade is an
  effect model instead (`sfx` models 88570 / 88571).

## The build (build.py)

- `WEAPON_EFFECTS`: per weapon model, the effects it plays. Each gets a replace row: stock in the stock types'
  columns, a copy tinted with the weapon's color (`fxr/build_fxrs.mjs`, at + 9,700,000) in the custom ones.
- `WEAPON_BLADE_MATERIALS`: the weapons whose blade is a material. Their part (`wp_l_0221.partsbnd.dcx` + `_l`) is
  copied as model 9xxx (`wp_l_9221`), stock except its file name and its blade material reference
  (`P[WP_9220]_Laser`); their textures are copied too (`wp_9221.tpf.dcx` + `_l`: the game loads them by number, and
  without them the body renders unpainted), and the material is copied into `allmaterial`.
- Light wave and WLT 101 blade models get neutral copies (`TINTED_MATERIAL_MODELS`), like the expansion models.
- Rows are appended to the replace table (`Param.add_rows`: it has no paramdef, and its rows aren't sorted).

## The DLL (dll/src/weapons.rs)

- **Model swap**: the AC parts loader `0x1410d8460(parts, assembly, ...)` builds each slot's file name with
  `0x1415d0080(wstring *out, param row, ...)` (prefix + `%04d` of `equipModelId`). A hook on the loader keeps the
  parts object being loaded in a thread-local; a hook on the name builder, for the player's parts object
  (`[player ChrIns + 0x47e8]`), turns `WP_L_0221` into `WP_L_9221`. Other ACs and the garage's preview models load
  the stock part.
- **Blade colors**: the material repository `[0x144e1cc38]` (`CS::MtdRepository`), `0x141344fe0(repository,
  L"P[WP_9220]_Laser")` -> `MtdResCap`, the material at `+0x78`; `0x142300730(material, L"..._color_6")` -> the
  parameter, its rgba at `+0x40` (intensity at `+0x50`); then `0x1423020a0(material)` rebuilds the material's
  constant buffer, as the game's own matbin reload does. Each blade color takes the chosen color at its stock
  brightness (brightest channel); intensities stay.
- **Effect colors**: served as external values, nine per weapon from 900000 + 10 × its model, laid out like the
  thruster colors (`dll/src/ext_values.rs`).
- **Settings**: the WEAPON VFX row (`rows::claim_dynamic`). Every melee weapon has its own color in the row's paint
  section (`WEAPONS` order: colors 0-5, then pattern colors 0-3; docs/paint-rows.md). The panel shows the two equipped
  ones, labelled with the game's own names (item text 11 by EquipParamWeapon ID, through the message repository
  `0x14157d750`): the left-arm melee weapon and the left-back one, read from the player's assembly `[ChrIns + 0x47e0]`,
  weapons at `+0xF4 + slot * 4` (slot 0 left arm, 2 left back; the game's getter `0x1406f5890`). The update loop serves
  the equipped weapons' colors to their models (effects, blade material). The preview's highlight flashes the weapons'
  units (hook on `0x14102c4c0`; docs/paint-rows.md, "The highlight").
- **Effects left stock:** the lance's exhaust (991309, built like the AC's own exhaust, 410031).
