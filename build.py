"""Builds the mod from an installed copy of Armored Core VI 1.09.1.

    python build.py --game "<...>/ARMORED CORE VI FIRES OF RUBICON/Game" --witchy "<...>/WitchyBND.exe"

Needs node/npm, cargo (stable) and WitchyBND 3.x. Writes package/: the mod folder (regulation, custom flames, the
Paint movie with the extra rows, the prepared HUD movie), the DLL and the me3 profile.
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
import xml.etree.ElementTree as ET

ROOT = os.path.dirname(os.path.abspath(__file__))
WORK = os.path.join(ROOT, "build")
PACKAGE = os.path.join(ROOT, "package")

# Thruster color type N uses column sfxIdN of the generator-dependent effect tables. Stock types 0-2 (orange, blue,
# Coral); the flame types are custom types 3-6 (unused in vanilla): combustion, current, Coral, ALLMIND, whose flames
# are bundles of copies tinted with the player's color (fxr/build_fxrs.mjs). Only the player's AC gets them: the DLL
# hooks the type (dll/src/columns.rs). Elsewhere a custom type uses what stock type CUSTOM_TYPES_STOCK[k] does.
STOCK_COLOR_TYPES = (0, 1, 2)
FIRST_CUSTOM_TYPE = 3
CUSTOM_TYPES_STOCK = (0, 1, 2, 1)
# GeneratorTypeSfxIdRplace_PC rows with custom columns: the core expansions' (assault armor, pulse armor, pulse
# protection...) and the melee weapons' (WEAPON_EFFECTS); dll/src/columns.rs CUSTOM_COLUMN_EFFECTS mirrors it.
EXPANSION_ROWS = range(990000, 1000000)
# Expansion models (RichModels) whose material has its own color (light blue, 83038 orange), which a tint can only
# multiply: the copies use a neutral version, model and material renamed s8... -> s9... (s88232 -> s98232; same length,
# so the model's material reference is patched in place), with the material's colors greyed to their luminance like
# the expansion copies' own colors (fxr/build_fxrs.mjs recolor).
TINTED_MATERIAL_MODELS = (83038, 88231, 88232, 88235,
                          83040, 83042, 83140, 83142,  # MOONLIGHT / REDSHIFT light waves
                          88570, 88571)  # WLT 101's blade
# Thruster effect rows that stay stock: the exhaust fumes (smoke only, no flame).
STOCK_FLAME_ROWS = {410031}

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
    610: [991300, 991301, 991302, 991305, 991307, 991308, 991309, 991310, 991311, 991312, 991315],  # laser lance
    910: [991800, 991805, 991806, 991807, 991808, 991809, 991810, 991811, 991815, 991816, 991817],  # stun baton
    1210: [991700, 991701, 991702, 991703, 991710, 991711, 991712, 991713, 991714, 991715],  # plasma thrower
}
# Weapons whose blade is a material of their part (fixed colors): the player's AC loads a copy of the part, model
# 0xxx -> 9xxx (the DLL renames it as it loads; dll/src/weapons.rs WEAPONS lists the copies' materials), with copies
# of its textures and of this material (P[WP_0...] -> P[WP_9...]), whose colors the DLL sets.
WEAPON_BLADE_MATERIALS = {
    211: "P[WP_0210]_Laser", 221: "P[WP_0220]_Laser", 241: "P[WP_0240]_Laser", 250: "P[WP_0250]_Laser",
    251: "P[WP_0251]_Laser", 411: "P[WP_0410]_Laser", 610: "P[WP_0610]_Laser",
}


def player_copy(name):
    """A weapon part / texture / material name -> its copy's (same length): the model number's leading 0 -> 9."""
    copy = re.sub(r"(?i)(wp_(?:l_)?)0(\d{3})", r"\g<1>9\g<2>", name, count=1)
    assert copy != name, f"no model number in {name}"
    return copy


def rename_in_file(path, renames):
    """Same-length renames inside a binary file, as UTF-16."""
    data = open(path, "rb").read()
    for old, new in renames:
        data = data.replace(old.encode("utf-16-le"), new.encode("utf-16-le"))
    open(path, "wb").write(data)


def copy_weapon_parts(game, work):
    """Copies the weapon parts the player's AC loads instead (WEAPON_BLADE_MATERIALS; part + LOD), for unpacking."""
    return [shutil.copy(os.path.join(game, "parts", f"wp_l_{model:04d}{suffix}.partsbnd.dcx"), work)
            for model in WEAPON_BLADE_MATERIALS for suffix in ("", "_l")]


def make_weapon_copies(game, work, parts, material_dir):
    """The copies of the unpacked `parts`: their blade material references and file names renamed, the materials and
    textures copied. Returns the part files the repack writes, and the texture copies."""
    for model, material in WEAPON_BLADE_MATERIALS.items():
        renames = [(material, player_copy(material))]
        for part in [p for p in parts if os.path.basename(p).startswith(f"wp_l_{model:04d}")]:
            folder = unpacked(part)
            for flver in [f for f in os.listdir(folder) if f.lower().endswith(".flver")]:
                rename_in_file(os.path.join(folder, flver), renames)
            manifest = os.path.join(folder, "_witchy-bnd4.xml")
            text = open(manifest, encoding="utf-8-sig").read()
            name = os.path.basename(part)
            assert f"<filename>{name}</filename>" in text, f"{manifest}: unexpected file name"
            open(manifest, "w", encoding="utf-8-sig").write(text.replace(name, player_copy(name)))
        # The material names itself once, in its source path.
        copy = shutil.copy(os.path.join(material_dir, "Parts", material + ".matbin"),
                           os.path.join(material_dir, "Parts", player_copy(material) + ".matbin"))
        rename_in_file(copy, renames)
    built = [os.path.join(work, player_copy(os.path.basename(p))) for p in parts]
    textures = [shutil.copy(os.path.join(game, "parts", name), os.path.join(work, player_copy(name)))
                for name in (f"wp_{model:04d}{suffix}.tpf.dcx" for model in WEAPON_BLADE_MATERIALS for suffix in ("", "_l"))]
    return built, textures

FIELD_SIZES = {'s8': 1, 'u8': 1, 'dummy8': 1, 's16': 2, 'u16': 2, 's32': 4, 'u32': 4, 'f32': 4, 'angle32': 4,
               'b32': 4, 'f64': 8, 'fixstr': 1, 'fixstrW': 2}
FIELD_FORMATS = {'s8': '<b', 's32': '<i'}


class Param:
    """Raw .param file, fields located through WitchyBND's Paramdex defs (or `layout`, a list of (type, name) for
    params without one), edited in place."""

    def __init__(self, path, def_path=None, layout=None):
        self.path = path
        self.data = bytearray(open(path, "rb").read())

        # Field definitions like "s32 sfxId0", "u8 flag:1" (a bitfield) or "dummy8 pad[4]" (an array).
        if layout:
            definitions = [f"{typ} {name}" for typ, name in layout]
        else:
            definitions = [field.get("Def") for field in ET.parse(def_path).getroot().iter("Field")]

        # Lay the fields out in order. Consecutive bitfields of the same type share one storage unit until it's full.
        self.fields = {}  # name -> (offset in the row, type)
        offset = 0
        bitfield_unit_size = None  # size of the storage unit the current run of bitfields is filling
        bits_used = 0
        for definition in definitions:
            typ, name, bits, count = re.match(r"(\w+)\s+(\w+)(?::(\d+))?(?:\[(\d+)\])?", definition).groups()
            size = FIELD_SIZES[typ]
            if bits:
                fits_in_current_unit = bitfield_unit_size == size and bits_used + int(bits) <= size * 8
                if fits_in_current_unit:
                    bits_used += int(bits)
                else:
                    bitfield_unit_size = size
                    bits_used = int(bits)
                    offset += size
                continue
            bitfield_unit_size = None
            self.fields[name] = (offset, typ)
            offset += size * (int(count) if count else 1)
        row_size = offset

        # The row table: row_count entries of 0x18 bytes from 0x40, each an s32 row ID, 4 bytes of padding and the u64
        # offset of the row's data.
        self.read_rows()

        # AC6 paramdefs can be stale: a def whose size doesn't match the rows would put fields at wrong offsets.
        starts = sorted(self.rows.values())
        strides = {b - a for a, b in zip(starts, starts[1:])}
        assert strides <= {row_size}, f"{os.path.basename(path)}: the layout has {row_size:#x}-byte rows, the param {strides}"

    def get(self, row, field):
        offset, typ = self.fields[field]
        return struct.unpack_from(FIELD_FORMATS[typ], self.data, self.rows[row] + offset)[0]

    def set(self, row, field, value):
        offset, typ = self.fields[field]
        struct.pack_into(FIELD_FORMATS[typ], self.data, self.rows[row] + offset, value)

    def read_rows(self):
        # The row table: row_count entries of 0x18 bytes from 0x40, each an s32 row ID, 4 bytes of padding, the u64
        # offset of the row's data and the u64 offset of its name.
        count = struct.unpack_from("<H", self.data, 0xA)[0]
        self.rows = dict(struct.unpack_from("<i4xQ", self.data, 0x40 + i * 0x18) for i in range(count))  # ID -> data

    def add_rows(self, rows):
        """Appends rows ({row ID: data bytes}) to the param: their entries after the row table, their data after the
        last row's, every later offset shifted. Rows are unnamed (the stock rows' empty name)."""
        old = self.data
        count = len(self.rows)
        table_end = 0x40 + count * 0x18
        strings = struct.unpack_from("<I", old, 0)[0]  # the type name, then the row names: after all the data
        entry_shift = len(rows) * 0x18
        string_shift = entry_shift + sum(map(len, rows.values()))
        table = bytearray(old[0x40:table_end])
        for i in range(count):
            data_offset, name_offset = struct.unpack_from("<QQ", table, i * 0x18 + 8)
            struct.pack_into("<QQ", table, i * 0x18 + 8, data_offset + entry_shift, name_offset + string_shift)
        empty_name = struct.unpack_from("<Q", table, 0x10)[0]
        at = strings + entry_shift
        for row_id, data in rows.items():
            table += struct.pack("<i4xQQ", row_id, at, empty_name)
            at += len(data)
        self.data = bytearray(old[:0x40] + table + old[table_end:strings] + b"".join(rows.values()) + old[strings:])
        struct.pack_into("<I", self.data, 0, strings + string_shift)
        struct.pack_into("<H", self.data, 0xA, count + len(rows))
        struct.pack_into("<Q", self.data, 0x10, struct.unpack_from("<Q", old, 0x10)[0] + string_shift)  # type name
        struct.pack_into("<Q", self.data, 0x30, struct.unpack_from("<Q", old, 0x30)[0] + entry_shift)  # data start
        self.read_rows()

    def save(self):
        open(self.path, "wb").write(self.data)


def make_neutral_models(model_dir, material_dir, witchy):
    """The neutral versions of TINTED_MATERIAL_MODELS (model -> neutral model): each model copied with its material
    reference renamed, and that material copied with its colors greyed. Returns the model IDs' mapping."""
    neutral, materials = {}, []
    for model in TINTED_MATERIAL_MODELS:
        flver = open(os.path.join(model_dir, f"s{model}.flver"), "rb").read()
        # Its material, "s8xxxx_<...>.matxml" in UTF-16.
        references = set(re.findall(rb"s\x008\x00(?:\d\x00){4}(?:[^\x00]\x00)*?\.\x00m\x00a\x00t\x00x\x00m\x00l\x00", flver))
        assert len(references) == 1 and str(model)[0] == "8", f"model {model}: unexpected material references"
        reference = references.pop()
        renamed = reference[:2] + "9".encode("utf-16-le") + reference[4:]
        neutral[model] = model + 10000
        open(os.path.join(model_dir, f"s{neutral[model]}.flver"), "wb").write(flver.replace(reference, renamed))
        source, target = (os.path.join(material_dir, "sfx", name.decode("utf-16-le").replace(".matxml", ".matbin"))
                          for name in (reference, renamed))
        if target not in materials:
            shutil.copy(source, target)
            materials.append(target)
    witchy(materials, [m + ".xml" for m in materials])
    for material in materials:
        text = open(material + ".xml", encoding="utf-8-sig").read()

        def grey(param):
            block = param.group(0)
            name = re.search(r"<Name>(.*?)</Name>", block).group(1)
            if not re.search(r"<Type>Float[345]</Type>", block) or not ("color" in name.lower() or name == "g_GlowScale"):
                return block
            values = re.findall(r"<float>(.*?)</float>", block)
            r, g, b = (float(v) for v in values[:3])
            grey = 0.2126 * r + 0.7152 * g + 0.0722 * b
            floats = iter([grey] * 3 + [float(v) for v in values[3:]])
            return re.sub(r"<float>.*?</float>", lambda _: f"<float>{next(floats):g}</float>", block)

        text = re.sub(r"<Param>.*?</Param>", grey, text, flags=re.S)
        text = re.sub(r"(<SourcePath>)s8", r"\1s9", text)
        open(material + ".xml", "w", encoding="utf-8-sig").write(text)
    witchy([m + ".xml" for m in materials], materials)
    for material in materials:
        os.remove(material + ".xml")
    return neutral


def unpacked(path):
    # WitchyBND's folder naming, next to the file: regulation.bin -> regulation-bin, x.ffxbnd.dcx -> x-ffxbnd-dcx-wffxbnd
    # (the same for matbinbnd)
    name = os.path.basename(path)
    kind = re.search(r"\.(ffxbnd|matbinbnd)\.dcx$", name)
    folder = name.replace(".", "-") + (f"-w{kind.group(1)}" if kind else "")
    return os.path.join(os.path.dirname(path), folder)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--game", required=True, help="the game's Game folder (with regulation.bin and sfx/)")
    parser.add_argument("--witchy", required=True, help="WitchyBND.exe (3.x)")
    args = parser.parse_args()
    defs = os.path.join(os.path.dirname(args.witchy), "Assets", "Paramdex", "AC6", "Defs")

    # The DLL doesn't depend on anything below: build it meanwhile.
    cargo = subprocess.Popen(["cargo", "+stable", "build", "--release"], cwd=os.path.join(ROOT, "dll"))
    # If a later step fails, don't leave it compiling (and holding the target dir) after the build exits.
    atexit.register(lambda: cargo.poll() is None and cargo.kill())

    def witchy(inputs, outputs):
        """One WitchyBND run over all inputs (each launch is slow). It exits 0 even when it does nothing, so check
        that every expected output was (re)written."""
        before = {out: os.path.getmtime(out) if os.path.exists(out) else None for out in outputs}
        subprocess.run([args.witchy, "-s", *inputs], check=True)
        stale = [out for out in outputs if not os.path.exists(out) or os.path.getmtime(out) == before[out]]
        assert not stale, f"WitchyBND didn't produce {stale}"

    shutil.rmtree(WORK, ignore_errors=True)
    os.makedirs(WORK)
    regulation = shutil.copy(os.path.join(args.game, "regulation.bin"), WORK)
    effects = shutil.copy(os.path.join(args.game, "sfx", "sfxbnd_commoneffects.ffxbnd.dcx"), WORK)
    materials = shutil.copy(os.path.join(args.game, "material", "allmaterial.matbinbnd.dcx"), WORK)
    archives = [regulation, effects, materials]
    # Only read: ALLMIND's thruster effects, for the ALLMIND flame type's extras (fxr/build_fxrs.mjs).
    allmind = shutil.copy(os.path.join(args.game, "sfx", "sfxbnd_m30.ffxbnd.dcx"), WORK)
    # The player's weapon part copies (make_weapon_copies), unpacked and repacked with the archives.
    weapon_parts = copy_weapon_parts(args.game, WORK)
    witchy([*archives, allmind, *weapon_parts], [unpacked(a) for a in [*archives, allmind, *weapon_parts]])

    params = unpacked(regulation)
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
    built_parts, weapon_textures = make_weapon_copies(args.game, WORK, weapon_parts, unpacked(materials))
    # The DLL serves a weapon's color on nine external values from EXT_BASE + 10 * its model, and lists the
    # weapons and their blade materials' copies itself.
    weapons_rs = open(os.path.join(ROOT, "dll", "src", "weapons.rs")).read()
    ext_base = int(re.search(r"const EXT_BASE: i32 = (\d+);", weapons_rs).group(1))
    for model in WEAPON_EFFECTS:
        material = WEAPON_BLADE_MATERIALS.get(model)
        listed = f"model: {model}," in weapons_rs and (not material or f'b"{player_copy(material)}\\0"' in weapons_rs)
        assert listed, f"weapons.rs doesn't list weapon {model} (or its blade material) as build.py does"
    weapon_inputs = [[ext_base + 10 * model, effects_of_weapon] for model, effects_of_weapon in WEAPON_EFFECTS.items()]
    json.dump({"flames": list(flame_rows.values()), "others": expansion_effects, "weapons": weapon_inputs,
               "neutralModels": neutral_models}, open(input_path, "w"))
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
    for sfx in [sfx for effects_of_weapon in WEAPON_EFFECTS.values() for sfx in effects_of_weapon]:
        assert sfx not in replace.rows, f"replace row {sfx} exists"
    replace.add_rows({sfx: struct.pack("<8i", *[sfx] * FIRST_CUSTOM_TYPE, *[-1] * (8 - FIRST_CUSTOM_TYPE))
                      for effects_of_weapon in WEAPON_EFFECTS.values() for sfx in effects_of_weapon})

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

    witchy([unpacked(a) for a in [*archives, *weapon_parts]], [*archives, *built_parts])

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
    os.makedirs(os.path.join(PACKAGE, "mod", "parts"))
    for part in [*built_parts, *weapon_textures]:
        shutil.copy(part, os.path.join(PACKAGE, "mod", "parts"))
    # The HUD movie with the table of its colors, for the HUD rows (dll/src/hud_movie.rs).
    subprocess.run([sys.executable, os.path.join(ROOT, "tools", "prepare_hud.py"),
                    os.path.join(args.game, "menu", "01_000_fe.gfx"),
                    os.path.join(PACKAGE, "mod", "menu", "01_000_fe.gfx")], check=True)
    shutil.copy(os.path.join(ROOT, "dll", "target", "release", "customization_plus.dll"), PACKAGE)
    shutil.copy(os.path.join(ROOT, "customization-plus.me3"), PACKAGE)
    print(f"Built {PACKAGE}")


if __name__ == "__main__":
    main()
