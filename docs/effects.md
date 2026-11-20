# Tinted effects: flames, expansions

How the GENERATOR row's colors and flame type reach the effects (Armored Core VI 1.09.1). The mechanisms are shared
with the melee weapons ([weapon-vfx.md](weapon-vfx.md)): the player's own columns in the generator-dependent effect
tables (`dll/src/columns.rs`) and the colors served to tinted copies (`dll/src/ext_values.rs`); the copies are built by
`fxr/build_fxrs.mjs`.

## Flames

The game keeps a flame per thruster effect and generator color type (combustion, current, Coral): 20
effects, a column per type. The regulation adds four columns, one per flame type, pointing at effects built by
`fxr/build_fxrs.mjs` that play recolored copies of that type's flame: one adding the color and one subtracting what
the color lacks (that's what makes dark colors work). Each time a character's thrusters fire, the game asks for its
generator's color type to pick the column; the DLL hooks that question and answers with your flame type's column
for your AC only, so every other AC, even with the same generator, keeps its stock flames.

## Expansions

The same column picks the core expansions' effects from a second table, where the regulation points
the new columns at recolored copies tinted by the expansion color. Assault Armor's blast is the one exception: it's a
bullet's effect, spawned by plain ID, so the DLL also hooks the bullet effect spawn and, for your own bullets only
(the bullet knows its owner), looks the ID up in that table too. Some expansion meshes get their color from their
material, which a tint can only multiply: the copies use neutral versions of those models, with their material's
colors greyed (the package's `material/allmaterial.matbinbnd.dcx` adds those materials).

## Flame types

ALLMIND is a boss, not an AC, with thruster effects of its own; its type plays the current flame
or the boss's effects (in `sfxbnd_m30`, scaled down for an AC). Each of those is drawn twice, once in the accent color and once in the
thruster color, weighted by the particle's age, and the two add up to the fade (an effect can only multiply in the
values the DLL serves, so one particle can't blend two of them; the copies share their random seeds, so they line up).

## Color

The DLL reads your color from the GENERATOR row and serves it to the flames as FXR external values, the
same way as [AC6-FXR-DLL-POC](https://github.com/lugia19/AC6-FXR-DLL-POC). Without the DLL, everything plays stock.

The DLL does all the color math, and the flames only multiply by what it serves. That matters: an FXR can only take
one external value, run it through a curve and multiply it in, so anything that depends on the whole color (how
bright it is overall, which decides how much the subtracting copy darkens) can't be done inside the FXR. The DLL
works out the add, subtract and plain amounts once per color change, and the FXR just reads them.
