"""Builds the mod from an installed copy of Armored Core VI 1.09.1.

    python build.py --game "<...>/ARMORED CORE VI FIRES OF RUBICON/Game" --witchy "<...>/WitchyBND.exe"

Needs node/npm, cargo (stable) and WitchyBND 3.x. Writes package/: the mod folder (our own effect and material archives, which
the DLL loads next to the game's: custom flames and other tinted effects, neutral models and materials; the shader
bundle with the weapon colors' keys, the Paint movie with the extra rows, the prepared HUD movie), the DLL and the me3
profile. The pieces are in tools/ (params.py, models.py, weapons.py,
shaders.py, witchy.py, build_gfx.py, prepare_hud.py) and fxr/build_fxrs.mjs. The shader bundle is slow to repack:
cached in cache/.
"""
import argparse
import atexit
import json
import os
import re
import shutil
import subprocess
import sys

from tools import shaders, weapons
from tools.models import make_neutral_models
from tools.params import Param
from tools.witchy import Witchy, empty_like, unpacked

ROOT = os.path.dirname(os.path.abspath(__file__))
WORK = os.path.join(ROOT, "build")
CACHE = os.path.join(ROOT, "cache")
PACKAGE = os.path.join(ROOT, "package")

# Thruster color type N uses column sfxIdN of the generator-dependent effect tables. Stock types 0-2 (orange, blue,
# Coral); the flame types are custom types 3-6 (unused in vanilla): combustion, current, Coral, ALLMIND, whose flames
# are bundles of copies tinted with the player's color (fxr/build_fxrs.mjs). Only the player's AC gets them: the DLL
# hooks the type (dll/src/columns.rs). Elsewhere a custom type uses what stock type CUSTOM_TYPES_STOCK[k] does.
STOCK_COLOR_TYPES = (0, 1, 2)
FIRST_CUSTOM_TYPE = 3
CUSTOM_TYPES_STOCK = (0, 1, 2, 1)
# GeneratorTypeSfxIdRplace_PC's core expansion rows (assault armor, pulse armor, pulse protection...).
EXPANSION_ROWS = range(990000, 1000000)
# Thruster effect rows that stay stock: the exhaust fumes (smoke only, no flame).
STOCK_FLAME_ROWS = {410031}
# Our own archives, holding only what we add, loaded by the DLL next to the game's (dll/src/archives.rs names them).
OWN_EFFECTS = os.path.join("sfx", "sfxbnd_customizationplus.ffxbnd.dcx")
OWN_MATERIALS = os.path.join("material", "customizationplus.matbinbnd.dcx")
# The game registers an effect archive's FXRs only up to the first entry ID >= this (0x1403f4540); WitchyBND numbers
# FXRs 0..N-1, before the other files.
MAX_EFFECT_ENTRIES = 99999


def write_generated(name, text):
    """A generated file of the DLL's (dll/src, committed so the DLL builds on its own), rewritten only on a change.
    Returns whether it changed."""
    path = os.path.join(ROOT, "dll", "src", name)
    if os.path.exists(path) and open(path).read() == text:
        return False
    open(path, "w", newline="\n").write(text)
    print(f"Updated {path}")
    return True


def build_dll():
    """cargo, in the background; killed if the build fails meanwhile (it would hold the target dir)."""
    cargo = subprocess.Popen(["cargo", "+stable", "build", "--release"], cwd=os.path.join(ROOT, "dll"))
    atexit.register(lambda: cargo.poll() is None and cargo.kill())
    return cargo


def file_names(folder):
    """The lowercased names of a folder's files, recursively (the game registers files by name)."""
    return {f.lower() for _, _, names in os.walk(folder) for f in names if not f.startswith("_witchy")}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--game", required=True, help="the game's Game folder (with regulation.bin and sfx/)")
    parser.add_argument("--witchy", required=True, help="WitchyBND.exe (3.x)")
    args = parser.parse_args()
    defs = os.path.join(os.path.dirname(args.witchy), "Assets", "Paramdex", "AC6", "Defs")

    witchy = Witchy(args.witchy)

    shutil.rmtree(WORK, ignore_errors=True)
    os.makedirs(WORK)
    # Only read: the params, the stock effects and materials (what our copies start from), and ALLMIND's thruster
    # effects for the ALLMIND flame type's extras (fxr/build_fxrs.mjs).
    regulation, effects, materials, allmind = stock = [
        shutil.copy(os.path.join(args.game, path), WORK)
        for path in ("regulation.bin", os.path.join("sfx", "sfxbnd_commoneffects.ffxbnd.dcx"),
                     os.path.join("material", "allmaterial.matbinbnd.dcx"), os.path.join("sfx", "sfxbnd_m30.ffxbnd.dcx"))]
    witchy(stock, [unpacked(a) for a in stock])
    own_effects = os.path.join(WORK, os.path.basename(OWN_EFFECTS))
    own_materials = os.path.join(WORK, os.path.basename(OWN_MATERIALS))
    own_effect_dir, own_model_dir, own_reslist_dir = empty_like(own_effects, unpacked(effects), "effect", "model",
                                                                "ResourceList")
    own_material_dir, = empty_like(own_materials, unpacked(materials), "sfx")

    params = unpacked(regulation)
    # The melee weapons (tools/weapons.py): their parts (only read), their animations and colored materials, what the
    # data says they play, and the DLL's table of them.
    weapon_models = weapons.models(params, defs)
    weapon_parts = weapons.copy_weapon_parts(args.game, WORK, weapon_models)
    witchy(weapon_parts, [unpacked(p) for p in weapon_parts])
    anibnds = weapons.animation_bundles(weapon_parts)
    matbins = weapons.material_files(os.path.join(unpacked(materials)))
    witchy([*anibnds, *matbins], [*map(unpacked, anibnds), *[m + ".xml" for m in matbins]])
    weapon_data = weapons.derive(params, defs, os.path.join(unpacked(effects), "effect"), weapon_models, anibnds)
    weapons.report(weapon_models, weapon_data)
    weapon_colors = weapons.material_colors(unpacked(materials))
    write_generated("weapon_table.rs", weapons.rust_table(weapon_models, weapon_data, weapon_colors))
    # The DLL's other generated table (effect_table.rs) comes after the effects are built: built again if it changes.
    cargo = build_dll()

    # The generator-dependent effect tables: row = an effect the game spawns, column t (sfxId<t>) = what it uses for
    # generator color type t, -1 = the row's own effect (e.g. row 1003010, the assault boost startup flare, has no
    # columns: one flare for every type). Thrusters: the flames, which get custom bundles. Replace (no paramdef, 8 s32
    # columns): everything else that depends on the generator (assault armor, pulse armor...).
    thrusters = Param(os.path.join(params, "ThrustersSfxIdParam_PC.param"), os.path.join(defs, "ThrustersSfxIdParam.xml"))
    replace = Param(os.path.join(params, "GeneratorTypeSfxIdRplace_PC.param"),
                    layout=[("s32", f"sfxId{t}") for t in range(8)])

    def effect_for(table, row, t):
        sfx = table.get(row, f"sfxId{t}")
        return sfx if sfx != -1 or row == 0 else row

    effect_dir = os.path.join(unpacked(effects), "effect")
    # Each row's flames for the three stock types (row 0 has none). A flame is one row's, for one type (or the same
    # for every type): its bundle ID can then derive from it.
    flame_rows = {}
    row_of = {}
    for row in sorted(thrusters.rows):
        flames = [effect_for(thrusters, row, t) for t in STOCK_COLOR_TYPES]
        if row in STOCK_FLAME_ROWS or -1 in flames:
            continue
        flame_rows[row] = flames
        for flame in flames:
            assert row_of.setdefault(flame, flames) == flames, f"stock flame {flame} is in rows with different flames"

    # The core expansions' effects, every stock type's (some rows have no effect file).
    expansion_effects = sorted({effect_for(replace, row, t) for row in replace.rows if row in EXPANSION_ROWS
                                for t in STOCK_COLOR_TYPES} & {int(f[1:10]) for f in os.listdir(effect_dir) if f.endswith(".fxr")})

    # Custom flames: a bundle per row and flame type, playing custom copies; the expansions' effects get a bundle each
    # (see fxr/build_fxrs.mjs).
    fxr_dir = os.path.join(ROOT, "fxr")
    shell = os.name == "nt"
    if not os.path.isdir(os.path.join(fxr_dir, "node_modules")):
        subprocess.run(["npm", "install"], cwd=fxr_dir, check=True, shell=shell)
    id_map_path = os.path.join(WORK, "id_map.json")
    input_path = os.path.join(WORK, "fxr_input.json")
    neutral_models = make_neutral_models(os.path.join(unpacked(effects), "model"), unpacked(materials), own_model_dir,
                                         own_material_dir, witchy)
    weapon_inputs, hit_inputs = weapons.fxr_inputs(weapon_models, weapon_data)
    json.dump({"flames": list(flame_rows.values()), "others": expansion_effects, "weapons": weapon_inputs,
               "hits": hit_inputs, "neutralModels": neutral_models}, open(input_path, "w"))
    subprocess.run(["node", "build_fxrs.mjs", effect_dir, own_effect_dir, id_map_path, input_path,
                    os.path.join(unpacked(allmind), "effect")], cwd=fxr_dir,
                   check=True, shell=shell)
    id_map = json.load(open(id_map_path))
    bundles_of = dict(zip(flame_rows, id_map["bundles"]))
    other_bundles = {int(stock): bundle for stock, bundle in id_map["otherBundles"].items()}
    # The copies use their sources' textures and models. WitchyBND would generate empty resource lists for them (fine
    # for the bundles, which only hold proxies).
    reslists = os.path.join(unpacked(effects), "ResourceList")
    for new_id, source_id in id_map["resources"].items():
        target = os.path.join(own_reslist_dir, f"f{int(new_id):09d}.ffxreslist")
        source_name = f"f{source_id:09d}.ffxreslist"
        source = os.path.join(reslists, source_name)
        if not os.path.exists(source):  # ALLMIND's effects are in their own bnd
            source = os.path.join(unpacked(allmind), "ResourceList", source_name)
        resource_list = open(source, "rb").read()
        for model, neutral in neutral_models.items():
            resource_list = resource_list.replace(f"s{model}.sib".encode(), f"s{neutral}.sib".encode())
        open(target, "wb").write(resource_list)

    # The custom types' columns, answered by the DLL's lookup hooks (the regulation stays stock): a flame row's
    # bundles in the thruster table, what the matching stock type uses elsewhere (as its bundle if it has one), and
    # rows of their own for the weapons' effects. A lookup returns the row itself for a row or column it lacks, so
    # values equal to the row are left out.
    weapon_rows = weapons.effect_rows(weapon_data)
    assert len(set(weapon_rows)) == len(weapon_rows), "an effect is played by two weapons (it has one color)"
    lines = ["// Generated by build.py from the game's effect tables and the custom copies: don't edit.",
             "// Included by columns.rs: per row, its custom types' effects (FIRST_CUSTOM_TYPE on), -1 = the row's own.", "",
             f"const CUSTOM_TYPES: usize = {len(CUSTOM_TYPES_STOCK)};", ""]
    for name, table, bundles, added in [("THRUSTER_COLUMNS", thrusters, bundles_of, []),
                                        ("REPLACE_COLUMNS", replace, {}, weapon_rows)]:
        assert not set(added) & set(table.rows), f"{name}: rows {sorted(set(added) & set(table.rows))} exist"
        entries = []
        for row in sorted([*table.rows, *added]):
            if row in table.rows:
                assert all(table.get(row, f"sfxId{FIRST_CUSTOM_TYPE + k}") == -1 for k in range(len(CUSTOM_TYPES_STOCK))), \
                    f"{name} row {row}: a custom column in use"
            values = []
            for k, t in enumerate(CUSTOM_TYPES_STOCK):
                if row in bundles:
                    sfx = bundles[row][k]
                elif row in table.rows:
                    sfx = effect_for(table, row, t)
                else:
                    sfx = row
                sfx = other_bundles.get(sfx, sfx)
                values.append(-1 if sfx == row else sfx)
            if any(value != -1 for value in values):
                entries.append(f"    ({row}, [{', '.join(map(str, values))}]),")
        lines += [f"const {name}: [(i32, [i32; CUSTOM_TYPES]); {len(entries)}] = [", *entries, "];", ""]
    if write_generated("effect_table.rs", "\n".join(lines)):
        cargo.wait()
        cargo = build_dll()

    # The THRUSTERS row's choices (dll/src/thrusters.rs FLAME_TYPES): "generator's own" and the custom flame types.
    flame_types = 1 + len(CUSTOM_TYPES_STOCK)
    labels = re.search(r"const FLAME_TYPES: \[&str; (\d+)\]", open(os.path.join(ROOT, "dll", "src", "thrusters.rs")).read())
    assert int(labels.group(1)) == flame_types, f"thrusters.rs names {labels.group(1)} flame types, the params have {flame_types}"

    # Our files are additions: the first archive loaded owns a name, so none may share one with a stock file.
    for ours, theirs in [(own_effects, effects), (own_materials, materials)]:
        clashes = file_names(unpacked(ours)) & file_names(unpacked(theirs))
        assert not clashes, f"{os.path.basename(ours)}: {sorted(clashes)[:5]} are stock files"
    effect_count = len(os.listdir(own_effect_dir))
    assert effect_count < MAX_EFFECT_ENTRIES, f"{effect_count} effects: the game registers fewer than {MAX_EFFECT_ENTRIES}"
    witchy([unpacked(own_effects), unpacked(own_materials)], [own_effects, own_materials])
    shader_bundle = shaders.patched_bundle(args.game, CACHE, weapons.shader_keys(weapon_colors), witchy)

    assert cargo.wait() == 0, "cargo build failed"

    shutil.rmtree(PACKAGE, ignore_errors=True)
    os.makedirs(os.path.join(PACKAGE, "mod", "menu"))
    # The Paint movie with the extra rows' clips (dll/src/rows.rs).
    subprocess.run([sys.executable, os.path.join(ROOT, "tools", "build_gfx.py"),
                    os.path.join(args.game, "menu", "02_coloring.gfx"),
                    os.path.join(PACKAGE, "mod", "menu", "02_coloring.gfx")], check=True)
    for source, target in [(own_effects, OWN_EFFECTS), (own_materials, OWN_MATERIALS), (shader_bundle, shaders.BUNDLE)]:
        os.makedirs(os.path.join(PACKAGE, "mod", os.path.dirname(target)), exist_ok=True)
        shutil.copy(source, os.path.join(PACKAGE, "mod", target))
    # The HUD movie with the table of its colors, for the HUD rows (dll/src/hud_movie.rs).
    subprocess.run([sys.executable, os.path.join(ROOT, "tools", "prepare_hud.py"),
                    os.path.join(args.game, "menu", "01_000_fe.gfx"),
                    os.path.join(PACKAGE, "mod", "menu", "01_000_fe.gfx")], check=True)
    shutil.copy(os.path.join(ROOT, "dll", "target", "release", "customization_plus.dll"), PACKAGE)
    shutil.copy(os.path.join(ROOT, "customization-plus.me3"), PACKAGE)
    shutil.copy(os.path.join(ROOT, "launch.bat"), PACKAGE)
    print(f"Built {PACKAGE}")


if __name__ == "__main__":
    main()
