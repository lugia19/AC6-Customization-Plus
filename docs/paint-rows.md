# The extra Paint rows

How `dll/src/rows.rs` and `tools/build_gfx.py` add rows to the Paint screen. Armored Core VI 1.09.1; addresses are
for that exe. (This part was a separate library, NativePaint, until the HUD rows moved into this mod and it had no
other user.)

## The Paint list

`menu/02_coloring.gfx`'s list (sprite 268, ColorAndMaterialList) has one clip per row, `Item_0_0`..`Item_8_0`. The
screen fills a row vector and the list shows one clip per entry, in order; clips past the vector's size are hidden.

The row builder (`0x140f418e9`.., not a function in Ghidra: Arxan stubs split it) adds:

| Row | Kind (row `+0x98`) | Clip symbol |
|---|---|---|
| COLOR SET | 6 | 247 |
| one per color slot 0-5 (label = text 210111 + slot) | the slot | 244 |
| PATTERN | 7 | 235 |
| WEATHERING | 8 | 230 |

The clip symbols differ in their children (244 has `Color`, 235 has `Color_0..3` and `Pattern`, 230 has
`DegradeEffect`), so a row has to land on a clip of its own type. That's why the extra rows go after WEATHERING: put
anywhere earlier, the stock rows would shift onto the wrong clips whenever the number of extra rows changes.

The color rows come from a loop over the slot list (`0x14045d300`, a static {0..5}). For each slot the loop asks
`0x14045dc40` for its label and `0x14045de80` whether it's a normal slot:

- **Normal** (0-4): the row is built inline in the loop. Confirming it opens a Color / Material panel.
- **Not normal** (5, DEVICE): the row is built by `0x140f3b1c0(screen, list, slot, label)`. Confirming it opens the
  color picker directly.

Confirming a row asks `0x140f40d10` for the slots its editor edits: kind k -> {k}, color set -> all six, anything
else -> none.

The panel of a normal row comes from `0x140f43610(list, out, row kind)`: a switch with cases for kinds 0-4, each
calling the panel builder `0x140f49590(.., .., slot)` with its slot as a constant (and nothing for 5). The panel
has a Color entry (a DEVICE-type row, so the color picker) and a Material entry, which opens the material menu
`0x140f38320`.

Materials are rows of the regulation's `PlayerMaterialPresetParam` (0-35 for the color slots; 90-99, "Light Source
Intensity", for DEVICE, which the stock menus give no way to reach). The material menu lists the rows with
consecutive IDs starting at `0x14045da60(slot)`: a table with 0 for slots 0-4 and 90 for DEVICE, and nothing for
higher slots. A material has no name: the text shown for it is made from its numbers and fixed words, by
`0x140f35c40(info, out)` in the menu (info: slot +0, param row +0x10, material +0x20) and `0x140e0cc30(out,
material)` in the panel.

## The extra rows

Rows with slots 9, 10, 11 (not 0-8: those are the kinds above), built by the game's own loop.

- **Building.** The builder is called at `0x140f418d0` (Arxan moved its first instructions elsewhere and split its
  body with jumps, so Ghidra has no function there, but it is entered and left like any other); a hook on it marks
  when it's running. Inside it, the hook on the slot list adds the claimed rows' slots, so the loop makes a row for
  each, right after DEVICE. Only there: other code fills six-color arrays from that list, and nothing else asks
  for it while the builder runs. `0x14045de80` says an extra slot is normal
  if its row has choices: such a row gets the panel, the others are color-only.
- **Moving.** The builder ends by handing its list to `0x140d3cb00(list, ...)`, which copies it into the list
  widget (so moving rows after the builder returns would be too late). The hook on that function, when the builder
  is running and the list is the Paint list as built (the extra rows' kinds where the loop put them), first moves
  the extra rows after WEATHERING, the way the row vector moves rows when it grows: copy-construct (`0x140c81610`)
  at the new place, destroy (`0x140c82e00`) the old one.
- **Label.** `0x14045dc40` normally returns text 210111 + slot; the builder and the color picker's header both ask
  it. For an extra slot the hook returns the row's label. The game makes a label with its text lookup
  (`0x140cae6e0`): it finds the text for an ID in the message files, then builds the label from it with
  `0x140cb2970(out, text, text ID)`. The hook calls that second function with the row's own text, so no
  message file is involved. A label holds the text's address, not a copy (the game's point into its message data),
  so the DLL keeps every text allocated forever; a freed text shows as three placeholder blocks.
- **Editing.** `0x140f40d10` returns the row's own slot for its kind.
- **Panel.** The switch in `0x140f43610` has no case for an extra row's kind, so for a row with choices the hook
  does what the color slots' cases do, with the row's slot: copy the list's panel argument (`0x140c92e20(temporary,
  list + 0x78)`), call the panel builder (`0x140f49590(argument, list + 8, slot)`), store the panel in the output
  and add a reference to it (`0x142923570(panel + 8)`). The builder destroys the temporary (seen at runtime: its
  vtable is the base class's, `0x1438df5a0`, when the builder returns).
- **Choices.** A row's choices are materials `build.py` adds to `PlayerMaterialPresetParam`: up to 36 consecutive
  rows (as many as the stock menu lists) from 200 for row 0, 300 for row 1, 400 for row 2 (above the stock
  materials and apart from each other, since the menu lists consecutive IDs). The material icons show the ID in a
  right-aligned text field; the one on the Color / Material panel's icon (182) is 500 twips wide and cuts a third
  digit off, so `tools/build_gfx.py` makes it as wide as the others (800). `0x14045da60` returns that
  first ID for the row's slot, so the menu lists exactly those; both text functions return the choice's label for
  a material in a row's range. The paint data's material reader and writer (`0x14045a980(paint, sections, slot)`,
  under the edit context's `0x140e0cbf0`, and `0x14045b070(paint, sections, slot, material)`) use the row's
  storage, like the colors.
  `tools/add_choice_materials.py` adds the rows to the regulation.
- **Color.** Under the edit context's get and set (`0x140e0c960`, called every frame per swatch, and `0x140e0d030`),
  which turn the selected part category into its sections, are the paint data's own color reader and writer:
  `0x14045a7f0(paint, out, sections, slot)` and `0x14045afc0(paint, sections, slot, color)`. Those two are hooked:
  for an extra slot they use the row's storage instead of the given sections, whichever category is selected. An
  extra slot never goes through as is (sections only have six colors, a seventh would land in the pattern bytes).

## Storage

The paint data is 14 sections of 0x44 bytes, saved and carried by designs as one 0x3b8 block:

| Offset | Field |
|---|---|
| +0x00 | header (0xff) |
| +0x04 | weathering |
| +0x08 | 6 colors (RGBA bytes) |
| +0x20 | 6 materials |
| +0x2c | pattern design, size |
| +0x30 | 4 pattern colors |

Sections: 0 head, 1 core, 2 R arm, 3 L arm, 4 legs, 5 R weapon, 8 L weapon, 11 the leftover Extension unit, 12 R
back, 13 L back. The part categories (`0x14045d960`) never write 6, 7, 9 and 10; 10's header gets written by something
else and Sundance's Extension parts mod uses it. Rows 0, 1, 2 use sections 7, 6, 9 (row 0 on 7 because paint-menu
0.1.0 stored its thruster color there): the color in color 0, the choice in material 0. The rest of those sections
is unused.

Why a row is AC-wide and can't be per part: a stock row's color lives in the selected part's own sections (slot k of
head, core...), which is what makes it per part. An extra row has no slot in those sections (they only have six
colors), so its one color lives in a section of its own and the selected part is ignored. Per-part values would
need a place per part: ten sections' worth, and only three are free.

The game writes alpha 0xFF into every color it stores (unused sections hold its default grey; old saves can have
black with alpha 0). A color picked in an extra row is stored with alpha 0xFE; anything else means "not picked" and
the row shows its default. A choice is stored as 0xFE00 + its index, not as the material's ID (the unused
sections hold the default material, 18); anything else, or an index past the row's choices, means the default.

The player's paint block, which `rows::color` and `rows::choice` read (`dll/src/paint_data.rs`): `[[[GameDataMan]+8]+0x788]+0x14C`
(GameDataMan's pointer is at `0x144e03a08`; the chain is null while loading).

## The movie

`tools/build_gfx.py` patches the stock movie; everything it doesn't add stays byte-identical.

The list's panel is a fixed size: a solid rectangle (shape 129, scaled) and a texture (sprite 130) masked by a
rectangle (shape 217). It can't grow with the row count, so the extra rows get their own panel 240 twips below it,
made of slices: each extra row's clip is a copy of the color row sprite (244) with a piece of panel underneath (the
same rectangle and texture, the texture darker), reaching from the previous slice down to the margin below the row.
Hidden rows hide their slice, so the panel is as tall as the rows shown. The first row's clip also holds the
panel's header ("ENTIRE AC"): a copy of the rows' label text field (a DefineEditText, 228: font class `MenuFont_01`,
height, color, layout, initial text) with its own text, smaller and fainter. To fit the header and three rows above
the key guide, the extra rows are 950 twips apart, not the stock rows' 1000; they end about 50 twips above it.

The copies are added to the movie's SymbolClass table under the color row's ActionScript class
(`_02_Coloring_fla.Item_x_x__62`). Its frame scripts stop the row's animations; without the class a copy loops
through all its frames (it flickers) as soon as the game plays its grey-out animation, which it does while another
row is being edited.

Not done: the stock panel's corner dots have no counterpart on the extra panel, and it doesn't dim with the stock
panel while a row is being edited.

Testing a rebuilt movie needs a game restart: the game keeps the movie it loaded. With gfx-reloader in the profile,
a reload only gives a broken preview (it's an embedded panel), but a scene change after it (AC TEST in and out)
rebuilds the screen from the new file.

## DEVICE's brightness

The same three hooks that serve the extra rows also bring back the DEVICE row's hidden setting
(`dll/src/device.rs`): is-normal-slot answers true for DEVICE, so its row is built like MAIN..OTHER; the panel
switch gets the same hand-made panel as an extra row with choices, for slot 5; and both material text functions
label materials 90-99 "Brightness 10%" to "Brightness 450%": the material row's glow (a float at +8 of its
`PlayerMaterialPresetParam` row, 0.1 to 4.5, 1.0 for the default material 92) as a percentage. The game's own text
for them is "Light Source Intensity" and that float, in the menu only.

## Notes

Dropped or parked:
- Redirecting a color-only row's confirm to the panel (instead of building a normal row) crashes: the color picker
  opened from the panel runs `0x140f40a50`, which expects what a normal row captures.
- Reusing DEVICE's materials (90-99) as choices, with the first material set to 100 - N for N choices: worked, but a
  material then means different things per row, so the panel's text function (which only gets the material) would
  have to know which row is open. Own IDs per row avoid that.
- Per-part rows (idea, nothing needs it): a mode chosen at claim time where the row keeps one color per part
  section (10: head, core, arms, legs, hand weapons, back units, extension) instead of one for the AC. It fits in the
  row's own section, which has exactly ten color fields (6 colors + 4 pattern colors); the get/set hooks would use
  the selected part category instead of ignoring it, and the readers would take a part. Not checked: whether
  the game normalises the pattern color fields of the free sections on load. Not for thrusters: flames belong to
  the booster and sit on core and legs only, and the ownership chain gives the AC, not the body part.

Other findings:
- Paint writers next to the color one (`0x14045afc0`, +8): `0x14045aee0` pattern design (+0x2c), `0x14045af90`
  pattern size (+0x2d), `0x14045af30` pattern color (+0x30), `0x14045b020` weathering (+4), `0x14045b070` material
  (+0x20 + slot * 2; reached from `0x140e0d100`).
- Not identified: the third caller of the slot label function (`0x140f38c01`); the hook serves any caller.
