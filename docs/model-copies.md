# Model copies

Effect models whose color is in their material (`tools/models.py`); the weapon parts' colored materials are
[material-overrides.md](material-overrides.md).

Some effect models (RichModels) get their color from their material: light blue, or orange for 83038. A tint can only
multiply that. The copies ([tinted-copies.md](tinted-copies.md)) use neutral versions instead:

- the expansions' 83038, 88231, 88232, 88235;
- the MOONLIGHT / ML-REDSHIFT light waves' 83040, 83042, 83140, 83142;
- WLT 101's blade, 88570 / 88571 (its blade is an effect model, not part of the weapon).

Each is copied as model + 10000 (s8xxxx -> s9xxxx), its material renamed the same way (same length, so the model's
reference is patched in place), and that material copied into `allmaterial.matbinbnd.dcx` with its colors (and
`g_GlowScale`) greyed to their luminance, like the copies' own colors. The copies' resource lists point at them.
