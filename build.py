"""Builds the mod from an installed copy of Armored Core VI 1.09.1.

    python build.py --game "<...>/ARMORED CORE VI FIRES OF RUBICON/Game" --witchy "<...>/WitchyBND.exe"

Needs node/npm, cargo (stable) and WitchyBND 3.x. Writes package/: the mod folder (regulation, custom flames and other
tinted effects, materials, the shader bundle with the weapon colors' keys, the Paint movie with the extra rows, the
prepared HUD movie), the DLL and the me3 profile. The pieces are in tools/ (params.py, models.py, weapons.py,
shaders.py, witchy.py, build_gfx.py, prepare_hud.py) and fxr/build_fxrs.mjs. The shader bundle is slow to repack:
cached in cache/.
"""
import argparse
import atexit
import json
import os
import re
import shutil
import struct
import subprocess
import sys

from tools import shaders, weapons
from tools.models import make_neutral_models
from tools.params import Param
from tools.witchy import Witchy, unpacked

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
# GeneratorTypeSfxIdRplace_PC rows with custom columns: the core expansions' (assault armor, pulse armor, pulse
# protection...) and the melee weapons' (tools/weapons.py); dll/src/columns.rs CUSTOM_COLUMN_EFFECTS mirrors it.
EXPANSION_ROWS = range(990000, 1000000)
# Thruster effect rows that stay stock: the exhaust fumes (smoke only, no flame).
STOCK_FLAME_ROWS = {410031}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--game", required=True, help="the game's Game folder (with regulation.bin and sfx/)")
    parser.add_argument("--witchy", required=True, help="WitchyBND.exe (3.x)")
    args = parser.parse_args()
    defs = os.path.join(os.path.dirname(args.witchy), "Assets", "Paramdex", "AC6", "Defs")

    witchy = Witchy(args.witchy)

    shutil.rmtree(WORK, ignore_errors=True)
    os.makedirs(WORK)
    regulation = shutil.copy(os.path.join(args.game, "regulation.bin"), WORK)
    effects = shutil.copy(os.path.join(args.game, "sfx", "sfxbnd_commoneffects.ffxbnd.dcx"), WORK)
    materials = shutil.copy(os.path.join(args.game, "material", "allmaterial.matbinbnd.dcx"), WORK)
    archives = [regulation, effects, materials]
    # Only read: ALLMIND's thruster effects, for the ALLMIND flame type's extras (fxr/build_fxrs.mjs).
    allmind = shutil.copy(os.path.join(args.game, "sfx", "sfxbnd_m30.ffxbnd.dcx"), WORK)
    witchy([*archives, allmind], [unpacked(a) for a in [*archives, allmind]])

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
    table = weapons.rust_table(weapon_models, weapon_data, weapon_colors)
    table_path = os.path.join(ROOT, "dll", "src", "weapon_table.rs")
    if not os.path.exists(table_path) or open(table_path).read() != table:
        open(table_path, "w", newline="\n").write(table)
        print(f"Updated {table_path}")

    # The DLL doesn't depend on anything below: build it meanwhile.
    cargo = subprocess.Popen(["cargo", "+stable", "build", "--release"], cwd=os.path.join(ROOT, "dll"))
    # If a later step fails, don't leave it compiling (and holding the target dir) after the build exits.
    atexit.register(lambda: cargo.poll() is None and cargo.kill())
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
    neutral_models = make_neutral_models(os.path.join(unpacked(effects), "model"), unpacked(materials), witchy)
    weapon_inputs, hit_inputs = weapons.fxr_inputs(weapon_models, weapon_data)
    json.dump({"flames": list(flame_rows.values()), "others": expansion_effects, "weapons": weapon_inputs,
               "hits": hit_inputs, "neutralModels": neutral_models}, open(input_path, "w"))
    subprocess.run(["node", "build_fxrs.mjs", effect_dir, id_map_path, input_path,
                    os.path.join(unpacked(allmind), "effect")], cwd=fxr_dir,
                   check=True, shell=shell)
    id_map = json.load(open(id_map_path))
    bundles_of = dict(zip(flame_rows, id_map["bundles"]))
    other_bundles = {int(stock): bundle for stock, bundle in id_map["otherBundles"].items()}
    # The copies use their sources' textures and models. WitchyBND would generate empty resource lists for them (fine
    # for the bundles, which only hold proxies).
    reslists = os.path.join(unpacked(effects), "ResourceList")
    for new_id, source_id in id_map["resources"].items():
        target = os.path.join(reslists, f"f{int(new_id):09d}.ffxreslist")
        assert not os.path.exists(target), f"{new_id} collides with an existing effect"
        name = f"f{source_id:09d}.ffxreslist"
        source = os.path.join(reslists, name)
        if not os.path.exists(source):  # ALLMIND's effects are in their own bnd
            source = os.path.join(unpacked(allmind), "ResourceList", name)
        resource_list = open(source, "rb").read()
        for model, neutral in neutral_models.items():
            resource_list = resource_list.replace(f"s{model}.sib".encode(), f"s{neutral}.sib".encode())
        open(target, "wb").write(resource_list)

    # The weapons' effects get rows (stock), whose custom columns the loop below fills with their tinted copies.
    weapon_rows = weapons.effect_rows(weapon_data)
    assert len(set(weapon_rows)) == len(weapon_rows), "an effect is played by two weapons (it has one color)"
    for sfx in weapon_rows:
        assert sfx not in replace.rows, f"replace row {sfx} exists"
    replace.add_rows({sfx: struct.pack("<8i", *[sfx] * FIRST_CUSTOM_TYPE, *[-1] * (8 - FIRST_CUSTOM_TYPE))
                      for sfx in weapon_rows})

    # Params: the custom types' columns, a flame row's bundles in the thruster table, what the matching stock type uses
    # everywhere else (e.g. a Coral flame type gets the Coral expansion effects), as its bundle if it has one.
    for table, bundles in [(thrusters, bundles_of), (replace, {})]:
        for row in table.rows:
            for k, t in enumerate(CUSTOM_TYPES_STOCK):
                column = f"sfxId{FIRST_CUSTOM_TYPE + k}"
                assert table.get(row, column) == -1, f"{os.path.basename(table.path)} row {row}: {column} in use"
                sfx = bundles[row][k] if row in bundles else effect_for(table, row, t)
                if sfx != -1:
                    table.set(row, column, other_bundles.get(sfx, sfx))
        table.save()

    # The THRUSTER row's choices (dll/src/generator.rs FLAME_TYPES): "generator's own" and the custom flame types.
    flame_types = 1 + len(CUSTOM_TYPES_STOCK)
    labels = re.search(r"const FLAME_TYPES: \[&str; (\d+)\]", open(os.path.join(ROOT, "dll", "src", "generator.rs")).read())
    assert int(labels.group(1)) == flame_types, f"generator.rs names {labels.group(1)} flame types, the params have {flame_types}"

    witchy([unpacked(a) for a in archives], archives)
    shader_bundle = shaders.patched_bundle(args.game, CACHE, weapons.shader_keys(weapon_colors), witchy)

    assert cargo.wait() == 0, "cargo build failed"

    shutil.rmtree(PACKAGE, ignore_errors=True)
    os.makedirs(os.path.join(PACKAGE, "mod", "sfx"))
    os.makedirs(os.path.join(PACKAGE, "mod", "menu"))
    # The Paint movie with the extra rows' clips (dll/src/rows.rs).
    subprocess.run([sys.executable, os.path.join(ROOT, "tools", "build_gfx.py"),
                    os.path.join(args.game, "menu", "02_coloring.gfx"),
                    os.path.join(PACKAGE, "mod", "menu", "02_coloring.gfx")], check=True)
    shutil.copy(regulation, os.path.join(PACKAGE, "mod"))
    shutil.copy(effects, os.path.join(PACKAGE, "mod", "sfx"))
    os.makedirs(os.path.join(PACKAGE, "mod", "material"))
    shutil.copy(materials, os.path.join(PACKAGE, "mod", "material"))
    os.makedirs(os.path.join(PACKAGE, "mod", os.path.dirname(shaders.BUNDLE)))
    shutil.copy(shader_bundle, os.path.join(PACKAGE, "mod", shaders.BUNDLE))
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
