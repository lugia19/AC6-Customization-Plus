"""Neutral copies of effect models whose material has its own color."""
import os
import re
import shutil

# Expansion models (RichModels) whose material has its own color (light blue, 83038 orange), which a tint can only
# multiply: the copies use a neutral version, model and material renamed s8... -> s9... (s88232 -> s98232; same length,
# so the model's material reference is patched in place), with the material's colors greyed to their luminance like
# the expansion copies' own colors (fxr/build_fxrs.mjs recolor).
TINTED_MATERIAL_MODELS = (83038, 88231, 88232, 88235,
                          83040, 83042, 83140, 83142,  # MOONLIGHT / REDSHIFT light waves
                          88570, 88571)  # WLT 101's blade


def matbin(reference):
    """A material's matbin file name, from a model's reference to it (UTF-16, "....matxml")."""
    return reference.decode("utf-16-le").replace(".matxml", ".matbin")


def make_neutral_models(model_dir, material_dir, out_model_dir, out_material_dir, witchy):
    """The neutral versions of TINTED_MATERIAL_MODELS (model -> neutral model), read from the stock unpacks and written
    to ours: each model copied with its material reference renamed, and that material copied with its colors greyed.
    Returns the model IDs' mapping."""
    neutral, materials = {}, []
    for model in TINTED_MATERIAL_MODELS:
        flver = open(os.path.join(model_dir, f"s{model}.flver"), "rb").read()
        # Its material, "s8xxxx_<...>.matxml" in UTF-16.
        references = set(re.findall(rb"s\x008\x00(?:\d\x00){4}(?:[^\x00]\x00)*?\.\x00m\x00a\x00t\x00x\x00m\x00l\x00", flver))
        assert len(references) == 1 and str(model)[0] == "8", f"model {model}: unexpected material references"
        reference = references.pop()
        renamed = reference[:2] + "9".encode("utf-16-le") + reference[4:]
        neutral[model] = model + 10000
        open(os.path.join(out_model_dir, f"s{neutral[model]}.flver"), "wb").write(flver.replace(reference, renamed))
        source = os.path.join(material_dir, "sfx", matbin(reference))
        target = os.path.join(out_material_dir, matbin(renamed))
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
