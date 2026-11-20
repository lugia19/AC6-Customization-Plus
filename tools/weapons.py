"""WEAPON VFX's build side: the melee weapons' effects, hit effects and colored materials, derived from the game's data,
the part copies the player's AC loads, and the DLL's table of them (dll/src/weapon_table.rs, generated)."""
import glob
import os
import re
import shutil
import struct
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field

from tools.params import Param
from tools.witchy import unpacked


@dataclass
class Weapon:
    """What the game data doesn't say: the default color of its effects (0xRRGGBB), its part's colored materials (blades,
    emissives; fixed colors, so the player's AC loads a copy of the part with copies of them), and effects to leave
    stock ({id: why})."""
    id: int  # EquipParamWeapon
    default: int
    materials: list = field(default_factory=list)
    stock: dict = field(default_factory=dict)


# In the DLL's order, which is also where their colors are saved (the paint section's ten color fields): never reorder.
WEAPONS = [
    Weapon(20010000, 0x99FFFF, ["P[WP_0210]_Laser"]),  # HI-32: BU-TT/A pulse blade
    Weapon(20020000, 0x4DB3FF, ["P[WP_0220]_Laser"]),  # Vvc-770LB laser blade
    Weapon(20000100, 0xFF4D4D, ["P[WP_0230]_LB_Emissive_Glow"]),  # WLT 101 (its blade is an effect model)
    Weapon(20040000, 0x4DB3FF, ["P[WP_0240]_Laser"]),  # VP-67LD laser dagger
    Weapon(20000200, 0x80FFCC, ["P[WP_0250]_Laser"]),  # MOONLIGHT
    Weapon(20000300, 0xFF4D4D, ["P[WP_0251]_Laser"]),  # ML-REDSHIFT
    Weapon(20030000, 0x66CCFF, ["P[WP_0410]_Laser"]),  # Vvc-774LS laser slicer
    Weapon(20050000, 0x4DB3FF, ["P[WP_0610]_Laser"],  # VE-67LLA laser lance
           stock={991309: "its exhaust, grey smoke like the AC's own (410031)"}),
    Weapon(20090000, 0xFFCC4D),  # VP-67EB stun baton
    Weapon(20070000, 0x7766FF, ["P[WP_1210]_Plasmawire"]),  # 44-143 HMMR
]

# A weapon's color is served on nine external values from EXT_BASE + EXT_BLOCK * its model (dll/src/ext_values.rs).
EXT_BASE = 900000
EXT_BLOCK = 10
# The effects the DLL can give the player's own columns (dll/src/columns.rs CUSTOM_COLUMN_EFFECTS).
CUSTOM_COLUMN_EFFECTS = range(990000, 1000000)
# Hit effects get a copy per weapon: the pair at stock + model * HIT_MODEL_STRIDE + HIT_OFFSETS, their bundle at
# + HIT_BUNDLE_OFFSET (dll/src/hits.rs swaps it in).
HIT_MODEL_STRIDE = 100000
HIT_OFFSETS = (0, 30000)
HIT_BUNDLE_OFFSET = 60000

# TAE event 96: spawn an effect (its ID first in the params).
TAE_SPAWN_EFFECT = 96
# The bullet fields holding effect IDs.
BULLET_EFFECT_FIELDS = ["sfxId_Bullet", "sfxId_Hit", "muzzleFlashSfxId", "muzzleFlashSfxId_2", "add_sfxId_Bullet",
                        "sfxId_ShootDown", "sfxId_AtAccel", "sfxId_AfterAccel", "sfxId_Delete", "sfxId_Ricochet",
                        "sfxId_Ricochet_Delete", "sfxId_OnLeaveBarrier", "sfxId_waitingExplosion"]
# Hit effects (0x140b9f390): HitEffectSfxConceptParam row = the surface; its column atk<material>_1 = the attack's own
# effect (_2: surface debris, left stock); that HitEffectSfxParam row's column by type and size. The surfaces whose
# attack effect we tint: terrain and an AC's body (shields, Pulse Armor, Coral and guards have their own ripples).
HIT_CONCEPTS = [10, 103]
MATERIALS = {0: "Iron", 1: "Fire", 2: "Stun", 3: "Emp", 4: "Ice", 5: "Energy_Green", 6: "None", 11: "Melange",
             12: "AssaultArmor", 20: "Pulse", 21: "Laser", 22: "Plusma"}
HIT_TYPES = ["Slash", "Blow", "Thrust", "Blast", "Bullet"]  # the code's order (0x14159f020), not the paramdef's
HIT_SIZES = ["S", "M", "L", "LL", "LLL"]
ATK_MATERIAL_OF_WEAPON = 255
IRON = 0


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


def models(regulation, defs):
    """Each weapon's model (equipModelId), in WEAPONS' order."""
    param = Param(os.path.join(regulation, "EquipParamWeapon.param"), os.path.join(defs, "EquipParamWeapon.xml"))
    return [param.get(weapon.id, "equipModelId") for weapon in WEAPONS]


def copy_weapon_parts(game, work, model_ids):
    """Copies, for unpacking: every weapon's part (its animations are read), and the LODs of those the player's AC loads a
    copy of. Returns the parts to copy (part + LOD) and the others."""
    copy = lambda name: shutil.copy(os.path.join(game, "parts", name), work)  # noqa: E731
    copied, read_only = [], []
    for weapon, model in zip(WEAPONS, model_ids):
        if weapon.materials:
            taken = glob.glob(os.path.join(game, "parts", f"*{player_copy(f'wp_{model:04d}')[3:]}*"))
            assert not taken, f"weapon {weapon.id}: its copy's model number is taken by {taken}"
            copied += [copy(f"wp_l_{model:04d}{suffix}.partsbnd.dcx") for suffix in ("", "_l")]
        else:
            read_only.append(copy(f"wp_l_{model:04d}.partsbnd.dcx"))
    return copied, read_only


def animation_bundles(parts):
    """The animation bundles in the unpacked parts (the LODs have none)."""
    return [path for part in parts for path in glob.glob(os.path.join(unpacked(part), "*.anibnd"))]


def material_files(material_dir):
    """Every listed material's matbin, for reading its parameters."""
    return [os.path.join(material_dir, "Parts", name + ".matbin") for weapon in WEAPONS for name in weapon.materials]


def make_weapon_copies(game, work, parts, model_ids, material_dir):
    """The copies of the unpacked `parts`: their colored material references and file names renamed, the materials and
    textures copied. Returns the part files the repack writes, and the texture copies."""
    for weapon, model in zip(WEAPONS, model_ids):
        renames = [(material, player_copy(material)) for material in weapon.materials]
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
                for weapon, model in zip(WEAPONS, model_ids) if weapon.materials
                for name in (f"wp_{model:04d}{suffix}.tpf.dcx" for suffix in ("", "_l"))]
    return built, textures


# ---- what each weapon's data says -------------------------------------------------------------------------------------

def tae_effects(anibnd):
    """The effects a weapon part's animations spawn (event 96). Events aren't parsed: an event's data is `u32 type,
    u32 pad, u64 offset of its params` with the params right after, so it's found where that offset points at itself
    + 16."""
    effects = set()
    for path in glob.glob(os.path.join(unpacked(anibnd), "**", "*.tae"), recursive=True):
        data = open(path, "rb").read()
        for at in range(0, len(data) - 24, 8):
            event_type, pad, params = struct.unpack_from("<IIQ", data, at)
            if event_type == TAE_SPAWN_EFFECT and pad == 0 and params == at + 16:
                effects.add(struct.unpack_from("<i", data, params)[0])
    return effects


class Data:
    """The regulation's params WEAPON VFX reads."""

    def __init__(self, regulation, defs):
        def param(name, definition):
            return Param(os.path.join(regulation, name + ".param"), os.path.join(defs, definition + ".xml"))
        self.weapons = param("EquipParamWeapon", "EquipParamWeapon")
        self.behaviors = param("BehaviorParam_PC", "BehaviorParam")
        self.attacks = param("AtkParam_Pc", "AtkParam")
        self.bullets = param("Bullet", "BulletParam")
        self.concepts = param("HitEffectSfxConceptParam", "HitEffectSfxConceptParam")
        self.hit_effects = param("HitEffectSfxParam", "HitEffectSfxParam")

    def variation(self, weapon):
        return self.weapons.get(weapon.id, "behaviorVariationId")

    def attacks_and_bullets(self, weapon):
        """The weapon's attacks, as (material, type, size), and its bullets (every bullet its behaviors fire, and the
        bullets those fire)."""
        variation = self.variation(weapon)
        weapon_material = self.weapons.get(weapon.id, "atkMaterial")
        attacks, bullets = [], set()

        def attack(row, bullet=None):
            """An attack row, or a bullet's: its material and type, and its size, are the bullet's own."""
            if row not in self.attacks.rows:
                return
            if bullet is not None:
                attacks.append((self.bullets.get(bullet, "Material_AttackMaterial"),
                                self.bullets.get(bullet, "Material_AttackType"), self.bullets.get(bullet, "atkPow_forSfx")))
                return
            own = self.attacks.get(row, "atkMaterial")
            material = weapon_material if own == ATK_MATERIAL_OF_WEAPON else own
            attacks.append((material, self.attacks.get(row, "atkType"), self.attacks.get(row, "atkPow_forSfx")))

        for row in [r for r in self.behaviors.rows if self.behaviors.get(r, "variationId") == variation]:
            ref_type, ref = self.behaviors.get(row, "refType"), self.behaviors.get(row, "refId")
            if ref_type == 0:
                attack(ref)
            elif ref_type == 1:
                todo = [ref]
                while todo:
                    bullet = todo.pop()
                    if bullet <= 0 or bullet in bullets or bullet not in self.bullets.rows:
                        continue
                    bullets.add(bullet)
                    attack(self.bullets.get(bullet, "atkId_Bullet"), bullet)
                    todo += [self.bullets.get(bullet, "HitBulletID"), self.bullets.get(bullet, "intervalCreateBulletId")]
        return attacks, bullets

    def hit_effect(self, concept, material, kind, size):
        """The attack's own hit effect on a surface (0: none, or physical: metal sparks, not the weapon's color)."""
        name = MATERIALS.get(material)
        if material == IRON or name is None or concept not in self.concepts.rows or not (0 <= kind < 5 and 0 <= size < 5):
            return 0
        row = self.concepts.get(concept, f"atk{name}_1")
        if row not in self.hit_effects.rows:
            return 0
        return self.hit_effects.get(row, f"{HIT_TYPES[kind]}_{HIT_SIZES[size]}")


def derive(regulation, defs, effect_dir, model_ids, anibnds):
    """Per weapon: its effects (its animations' and its bullets'), and its hit effects."""
    data = Data(regulation, defs)
    exists = lambda sfx: os.path.exists(os.path.join(effect_dir, f"f{sfx:09d}.fxr"))  # noqa: E731
    derived = []
    for weapon, model in zip(WEAPONS, model_ids):
        anibnd = next(a for a in anibnds if f"_{model:04d}" in os.path.basename(a))
        attacks, bullets = data.attacks_and_bullets(weapon)
        effects = tae_effects(anibnd) | {data.bullets.get(b, f) for b in bullets for f in BULLET_EFFECT_FIELDS}
        effects = {sfx for sfx in effects if sfx > 0}
        outside = sorted(sfx for sfx in effects if sfx not in CUSTOM_COLUMN_EFFECTS)
        assert not outside, f"weapon {weapon.id} plays effects the DLL can't give columns: {outside}"
        missing = sorted(sfx for sfx in effects if not exists(sfx))
        hits = {data.hit_effect(c, *a) for c in HIT_CONCEPTS for a in attacks} - {0, -1}
        assert all(exists(sfx) for sfx in hits), f"weapon {weapon.id}: hit effects without a file"
        derived.append({
            "effects": sorted(effects - set(missing) - set(weapon.stock)),
            "missing": missing,
            "hits": sorted(hits),
            "variation": data.variation(weapon),
        })
    return derived


def material_colors(material_dir):
    """Each listed material's color parameters (a matbin converted to XML: its Float4 / Float5 params named color)."""
    colors = {}
    for weapon in WEAPONS:
        for name in weapon.materials:
            root = ET.parse(os.path.join(material_dir, "Parts", name + ".matbin.xml")).getroot()
            colors[name] = [p.findtext("Name") for p in root.iter("Param")
                            if p.findtext("Type") in ("Float4", "Float5") and "color" in p.findtext("Name").lower()]
            assert colors[name], f"{name}: no color parameters"
    return colors


# ---- the DLL's table, and build_fxrs.mjs's input --------------------------------------------------------------------

def rust_table(model_ids, derived, colors):
    """dll/src/weapon_table.rs: the weapons as the DLL sees them."""
    def wide(text):
        return f'&utf16(b"{text}\\0")'

    lines = [
        "// Generated by build.py (tools/weapons.py) from the game's data and its WEAPONS list: don't edit.",
        "// Included by weapons.rs.",
        "",
        f"const EXT_BASE: i32 = {EXT_BASE};",
        f"const EXT_BLOCK: i32 = {EXT_BLOCK};",
        f"const HIT_MODEL_STRIDE: i32 = {HIT_MODEL_STRIDE};",
        f"const HIT_BUNDLE_OFFSET: i32 = {HIT_BUNDLE_OFFSET};",
        "",
        f"const WEAPONS: [Weapon; {len(WEAPONS)}] = [",
    ]
    for weapon, model, info in zip(WEAPONS, model_ids, derived):
        materials = ", ".join(
            f"Material {{ name: {wide(player_copy(m))}, colors: &[{', '.join(wide(c) for c in colors[m])}] }}"
            for m in weapon.materials)
        lines += [
            "    Weapon {",
            f"        id: {weapon.id}, model: {model}, variation: {info['variation']}, default: 0x{weapon.default:06X},",
            f"        materials: &[{materials}],",
            f"        hits: &[{', '.join(map(str, info['hits']))}],",
            "    },",
        ]
    lines.append("];")
    return "\n".join(lines) + "\n"


def fxr_inputs(model_ids, derived):
    """build_fxrs.mjs's weapon inputs: [first external value, effects] per weapon, and [first external value, copy
    offsets, bundle offset, hit effects] per weapon."""
    weapons = [[EXT_BASE + EXT_BLOCK * model, info["effects"]] for model, info in zip(model_ids, derived)]
    hits = [[EXT_BASE + EXT_BLOCK * model, [model * HIT_MODEL_STRIDE + o for o in HIT_OFFSETS],
             model * HIT_MODEL_STRIDE + HIT_BUNDLE_OFFSET, info["hits"]] for model, info in zip(model_ids, derived)]
    return weapons, hits


def effect_rows(derived):
    """The effects that get replace rows: every weapon's."""
    return [sfx for info in derived for sfx in info["effects"]]


def report(model_ids, derived):
    for weapon, model, info in zip(WEAPONS, model_ids, derived):
        extra = f", no file: {info['missing']}" if info["missing"] else ""
        print(f"weapon {weapon.id} (model {model}): {len(info['effects'])} effects, hits {info['hits']}{extra}")
