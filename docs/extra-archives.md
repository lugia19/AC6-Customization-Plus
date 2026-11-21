# Extra archives

How the mod adds effects and materials without replacing the game's archives (`dll/src/archives.rs`, `build.py`).
Every file the mod adds to them (its effect copies, their resource lists, the neutral models and materials of
[model-copies.md](model-copies.md)) is new: none replaces a stock file. So instead of shipping whole copies of
`sfxbnd_commoneffects` (73 MB) and `allmaterial`, which other mods ship too (me3 keeps one copy of a file), the mod
ships archives of its own and has the game load them next to the stock ones:

| Archive | Holds | Loaded |
|---|---|---|
| `sfx/sfxbnd_customizationplus.ffxbnd.dcx` | the effect copies and bundles, their resource lists, the neutral models (1,153 files) | with the game's common effects |
| `material/customizationplus.matbinbnd.dcx` | the neutral models' materials (7) | with `AllMaterial`, at boot |

Elden Ring does the same with its DLC (its stay-load step loads `SfxBnd_CommonEffects`, `_dlc01` and `_dlc02`; its
graphics step `AllMaterial`, `_DLC01`, `_DLC02` and `.devpatch` archives); AC6 has the same functions, called once.

## Effects

`InGameStayStep`'s load `0x141260380(step)` loads the common effects with the effect manager's
`handle load(manager [0x144e29820], L"sfxbnd:/SfxBnd_CommonEffects.ffxbnd", step + 0xb8)` (`0x141623c10`; `+0xb8` is
the step's load group, which `STEP_InGameStayLoadWait` waits on). A hook after it makes the same call with
`L"sfxbnd:/SfxBnd_CustomizationPlus.ffxbnd"`, which me3 serves as `sfx/sfxbnd_customizationplus.ffxbnd.dcx`. The step
runs once per process (it unloads only when the game itself winds down), and the mod keeps its archive for the
session (release would be `0x141623f20(manager, handle)`).

Loading an archive registers its files in global repositories by name (FXRs, resource lists, models, textures,
anims): the first archive to load a name owns it, a later one only adds a reference. An FXR finds its resource list,
and the list its models and textures, by name in those repositories, whichever archive they came from: so the
copies' resource lists can name stock textures and models in the common effects.

The game registers an archive's FXRs only up to the first entry with a BND ID of 99,999 or more (`0x1403f4540`).
WitchyBND numbers FXRs 0..N-1, before the other files (textures from 100000, models 200000, resource lists 400000);
`build.py` checks there are fewer than 99,999.

## Materials

The boot graphics step `0x14129dbf0` loads `Load<MtdbndFileCap>(CSFile [0x144e00c50], L"material:/AllMaterial.matbinbnd",
0)` (`0x1403d2870`). A hook after it loads `L"material:/CustomizationPlus.matbinbnd"` the same way (served as
`material/customizationplus.matbinbnd.dcx`), once (the step also runs for a debug material reload). The archive's
matbins register in the material repository by name, first wins, as above.

A model binds its materials once, by name, when it's created (`CSFlverResourceFactory`; missing ones fall back to
`M[A]` for the model's life). Ours load at boot, long before the stay load creates the effect models that use them.

## The build

The stock archives are only read: `fxr/build_fxrs.mjs` reads stock effects and writes its copies to our archive's
`effect` folder, `tools/models.py` reads stock models and materials and writes the neutral copies to ours, and the
resource lists go to ours too. Each archive's folder starts as an empty unpack with the stock archive's WitchyBND
manifest under our name (`own_archive`), and WitchyBND packs whatever is in it.
