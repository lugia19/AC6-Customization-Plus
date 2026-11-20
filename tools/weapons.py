"""WEAPON VFX's build side (dll/src/weapons.rs): the melee weapons' effects and the part copies the player's AC loads."""
import os
import re
import shutil

from tools.witchy import unpacked

# WEAPON VFX (dll/src/weapons.rs): melee weapons by model (EquipParamWeapon equipModelId), each with the effects it plays
# (type 96 events in its part's animations, its bullets' effects; effects that draw nothing left out). Each effect gets
# a replace row whose custom columns play a copy tinted with the weapon's color (only the player's AC uses them).
WEAPON_EFFECTS = {
    211: [991000, 991001, 991002, 991005, 991006, 991010, 991011, 991012, 991015],  # pulse blade (991003-4: no file)
    221: [991400, 991401, 991402, 991405, 991410, 991411, 991412, 991415],  # laser blade
    230: [992100, 992101, 992102, 992105, 992110, 992111],  # WLT 101 (its blade is an effect)
    241: [991200, 991201, 991202, 991203, 991204, 991205, 991206, 991207, 991211, 991212, 991215],  # laser dagger
    250: [991905, 991906, 991907, 991910, 991915, 991916, 991917],  # MOONLIGHT
    251: [992205, 992206, 992207, 992210, 992215, 992216, 992217],  # ML-REDSHIFT
    411: [991500, 991501, 991502, 991505, 991506, 991507, 991510, 991511, 991512, 991515, 991516],  # laser slicer
    610: [991300, 991301, 991302, 991305, 991307, 991308, 991310, 991311, 991312, 991315],  # laser lance (991309: exhaust)
    910: [991800, 991805, 991806, 991807, 991808, 991809, 991810, 991811, 991815, 991816, 991817],  # stun baton
    1210: [991700, 991701, 991702, 991703, 991710, 991711, 991712, 991713, 991714, 991715],  # plasma thrower
}
# Weapons with colored materials on their part (blades, emissives; fixed colors): the player's AC loads a copy of the
# part (player_copy; the DLL renames it as it loads; dll/src/weapons.rs WEAPONS lists the copies' materials), with
# copies of its textures and of these materials, whose colors the DLL sets.
WEAPON_MATERIALS = {
    211: ["P[WP_0210]_Laser"], 221: ["P[WP_0220]_Laser"], 230: ["P[WP_0230]_LB_Emissive_Glow"],
    241: ["P[WP_0240]_Laser"], 250: ["P[WP_0250]_Laser"], 251: ["P[WP_0251]_Laser"], 411: ["P[WP_0410]_Laser"],
    610: ["P[WP_0610]_Laser"], 1210: ["P[WP_1210]_Plasmawire"],
}


def player_copy(name):
    """A weapon part / texture / material name -> its copy's (same length): the model number's leading digit d ->
    9 - d (0221 -> 9221, 1210 -> 8210; no stock weapon model is 8000-8949 or 9014-9999)."""
    copy = re.sub(r"(?i)(wp_(?:l_)?)([01])(\d{3})", lambda m: m[1] + str(9 - int(m[2])) + m[3], name, count=1)
    assert copy != name, f"no model number in {name}"
    return copy


def rename_in_file(path, renames):
    """Same-length renames inside a binary file, as UTF-16."""
    data = open(path, "rb").read()
    for old, new in renames:
        data = data.replace(old.encode("utf-16-le"), new.encode("utf-16-le"))
    open(path, "wb").write(data)


def copy_weapon_parts(game, work):
    """Copies the weapon parts the player's AC loads instead (WEAPON_MATERIALS; part + LOD), for unpacking."""
    return [shutil.copy(os.path.join(game, "parts", f"wp_l_{model:04d}{suffix}.partsbnd.dcx"), work)
            for model in WEAPON_MATERIALS for suffix in ("", "_l")]


def make_weapon_copies(game, work, parts, material_dir):
    """The copies of the unpacked `parts`: their colored material references and file names renamed, the materials and
    textures copied. Returns the part files the repack writes, and the texture copies."""
    for model, materials in WEAPON_MATERIALS.items():
        renames = [(material, player_copy(material)) for material in materials]
        for part in [p for p in parts if os.path.basename(p).startswith(f"wp_l_{model:04d}")]:
            folder = unpacked(part)
            for flver in [f for f in os.listdir(folder) if f.lower().endswith(".flver")]:
                rename_in_file(os.path.join(folder, flver), renames)
            manifest = os.path.join(folder, "_witchy-bnd4.xml")
            text = open(manifest, encoding="utf-8-sig").read()
            name = os.path.basename(part)
            assert f"<filename>{name}</filename>" in text, f"{manifest}: unexpected file name"
            open(manifest, "w", encoding="utf-8-sig").write(text.replace(name, player_copy(name)))
        # A material names itself once, in its source path.
        for material, copy_name in renames:
            copy = shutil.copy(os.path.join(material_dir, "Parts", material + ".matbin"),
                               os.path.join(material_dir, "Parts", copy_name + ".matbin"))
            rename_in_file(copy, [(material, copy_name)])
    built = [os.path.join(work, player_copy(os.path.basename(p))) for p in parts]
    textures = [shutil.copy(os.path.join(game, "parts", name), os.path.join(work, player_copy(name)))
                for name in (f"wp_{model:04d}{suffix}.tpf.dcx" for model in WEAPON_MATERIALS for suffix in ("", "_l"))]
    return built, textures


def fxr_inputs(weapons_rs):
    """build_fxrs.mjs's weapon input: [first external value, effects] per weapon. The DLL serves a weapon's color on nine
    external values from EXT_BASE + 10 * its model, and lists the weapons and their blade materials' copies itself:
    checked against these tables."""
    ext_base = int(re.search(r"const EXT_BASE: i32 = (\d+);", weapons_rs).group(1))
    for model in WEAPON_EFFECTS:
        materials = WEAPON_MATERIALS.get(model, [])
        listed = f"model: {model}," in weapons_rs and all(f'b"{player_copy(m)}\\0"' in weapons_rs for m in materials)
        assert listed, f"weapons.rs doesn't list weapon {model} (or its materials) as build.py does"
    return [[ext_base + 10 * model, effects_of_weapon] for model, effects_of_weapon in WEAPON_EFFECTS.items()]
