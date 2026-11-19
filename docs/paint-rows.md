# The extra Paint rows

How `dll/src/rows.rs`, `dll/src/panel.rs` and `tools/build_gfx.py` add rows to the Paint screen, and give them (and
DEVICE) panels. Armored Core VI 1.09.1; addresses are for that exe. (The row part was a separate library,
NativePaint, until the HUD rows moved into this mod and it had no other user.)

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

Items of ours, appended to the list the game's row builder made, after WEATHERING. No made-up slots: nothing in the
game is asked about them (research: `AC6_tools\Research\paint-popup\wide3\report_main_list.md`).

- **When.** The builder is called at `0x140f418d0(out, screen, &edit context, ...)` (Arxan moved its first
  instructions elsewhere and split its body with jumps, so Ghidra has no function there, but it is entered and left
  like any other); a hook on it keeps the edit context while it runs. The builder ends by handing its description to
  `0x140d3cb00(description, ...)`, which copies it into the list widget; the hook on that, while the builder runs
  and the description is the Paint list as built (9 rows, WEATHERING last), appends ours.
- **An item** is what the builder makes for a color row: a row from the label (`0x140d92d80`), appended with
  `0x140d3c600`, then:
  - kind (+0x98): 9, 10, 11, past the stock ones (0-5 slots, 6 COLOR SET, 7 PATTERN, 8 WEATHERING). Only the panel
    switch and the change checks read it; the edited-slots function gives unknown kinds an empty range, as it does
    PATTERN and WEATHERING.
  - refresh (+0x1b8): the swatch from the edit context's working paint block (so it follows the picker), and
    `Camouflage` set to "no pattern" (below). Its closure captures the edit context with a reference held, as the
    stock row refresh's `{ctx, slot}` does (`0x140f3f3a0`).
  - on-confirm (+0xf8): what a stock color row's returns (`0x143986608`, which captures nothing): a job, on the window
    the confirm is called with (the Paint screen), that plays the confirm sound, shows the panel's cursor and pushes
    the current panel (`screen + 0x2758`) on the child windows (`0x140f43500` -> `0x140f43d90`).
  - flags (+0x300) |= 8, as the builder does to every row that opens a panel (`0x140f41f20`): the list then greys out
    around the row while its panel has the focus (the grey-out is in the per-clip fill, `0x14100e4da..e50d`).
- **Label.** The game makes a label with its text lookup (`0x140cae6e0`): it finds the text for an ID in the message
  files, then builds the label with `0x140cb2970(out, text, text ID)`. We call that second function with the row's
  own text, so no message file is involved. A label holds the text's address, not a copy, so the DLL keeps every text
  allocated forever; a freed text shows as three placeholder blocks.
- **Panel.** The switch in `0x140f43610` has no case for our kinds; the hook returns a panel `dll/src/panel.rs`
  builds (below), with a reference added for the list as the cases do.
- **Refreshes run every update.** The list's per-clip fill (`0x14100e240`: label, then the refresh, on the stored
  item) runs when the window's `+0xc6d` is set; the Paint screen's update sets it every update (`0x140f42faa`).
- **The pattern overlay.** A swatch's `Camouflage` clip is never hidden: a hidden one broke the pattern thumbnails and
  overlays of the whole Paint screen. It is set the way the game does for a slot the pattern can't apply to
  (`0x140e0c660` answers 0 -> `0x140f452f0(clip, category, &0)`: its "no pattern" frame).

## The panels

A row's panel (Color / Material for MAIN..OTHER) is a list window: a description holding a header label and, per
entry, an on-confirm std::function (returns the job that opens what comes next) and a refresh std::function (fills
the entry's clip `Item_N_0` every frame, item + 0x1b8). The game's builder `0x140f49590` makes the description and
constructs the panel object on a panel clip (`0x140f47160(panel, clip handle, description, owner)`, 0x1da8 bytes).
`panel.rs` does the same with its own entries:

| Entry | Refresh | On confirm |
|---|---|---|
| Color | the swatch (`Color/Tile`, else `Color` itself, as the game's Color entry does; panel items have no Tile) and the RGB text; `Camouflage` (the pattern overlay) on "no pattern" | the game's color picker `0x140f37450(screen, out, a3 + 0x10, title, color, on-color)` with our title and callback |
| A choice (Flame type, Opacity) | the choice's name in `Value_0`; the material icon hidden | an option list, as the pattern's Size does, in our own 11-row list host (below) |
| Second color (Accent color, Warning color) | as Color | as Color |

- **Screens.** The panel state (`list`, the Paint screen + 0x2618) starts with the Paint screen (`list[0]`: its
  movie clips, MainContent at + 0x1da0, the edit context pointer at + 0x2600; set by the setup `0x140f3fd90`), and
  the owner (`list + 8`) gives the object the entries use through its virtual 2 (the edit context pointer at
  + 0x440; category + 0x28, working paint block + 0x40). The game's own entries get that same object.
- **a3.** An on-confirm's third argument is the panel window; + 0x10 is its child window stack, where the picker or
  list is pushed (as the game's entries pass it).
- **Option lists.** A description per open (title, one item per option, the current one selected), then the jobs as
  the Size list builds them: a restore job (`0x140f3c2b0`, a snapshot of the paint block), the list window
  (`0x140f382e0(desc, out, host)`, host = our tall list below, else the stock `screen + 0x3d8`), a branch restoring the snapshot on cancel (`0x1416c4660`), all inside
  the "sub-window open" wrapper (`0x140f3b490`). Options must be appended with `0x140c847e0`, the list-item add the
  game's lists use: appended as panel entries (`0x140d3c600`), confirming one leaves the list open. An option's
  on-confirm returns a job storing the choice (`0x140a63060(out, fn, {2, 0})`); with desc + 0xba0 set it also runs
  on every cursor move, which is the live preview.
- **Writes** go into the edit context's working paint block, so the picker and the list preview live and cancel
  puts the block back.
- **Clip.** The rows' panel is its own clip, `Color_Material_Color_Select`, a copy of the Color / Material panel
  with items color, color, material (`tools/build_gfx.py`), placed visible like the stock panels (the MainContent
  timeline replays its placements when a sub-window closes) and hidden at screen setup (`0x140f3fd90`, where the
  game hides its own three).
- **Tall list.** The stock list host `Text_ItemList` shows 3 items. `Text_ItemList_Tall` is a copy with 11 item
  slots: its `Base` has one frame per item count (the game picks clamp(count, 1, frames) in `0x140c76730`), frames
  4-11 stretch the background and a scale-9 copy of the outline, and it sits at the movie root 3000 twips higher
  than the stock host, using the full height. The DLL finds it under the root (`0x140c8a6f0`) and hides it at setup.
- Clip handles (0x60 bytes) point to themselves: they are built in place and never moved (a moved one crashed).

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
else and Sundance's Extension parts mod uses it. Rows 0, 1, 2 use sections 7, 6, 9 (row 0 on 7 because version
0.1.0 stored its thruster color there): the color in color 0, the second color in color 1, the choice in material
0. The rest of those sections is unused.

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

Two of the row hooks also bring back the DEVICE row's hidden setting (`dll/src/panel.rs`): is-normal-slot answers
true for DEVICE, so its row is built like MAIN..OTHER, and the panel switch gets a panel from `panel.rs` for slot
5: the game's own Color entry (`0x140f3b1c0(screen, desc, 5, label)`) and a Brightness entry doing what the game's
Material entry would for slot 5. Its confirm opens the stock material picker, `0x140f38320(screen, out, a3 + 0x10,
5)`, which lists materials 90-99 (`0x14045da60` gives 90 for DEVICE). Its refresh is the game's Material entry
refresh (`0x140f47e80`) redone for slot 5: the icon (`0x140f44fd0`, `0x140f45260(icon, material)`) and the menu's
text (`0x140f35c40`).

The labels: the game's text for these materials is text 210313, "Light Source Intensity", with the material row's
glow (a float at +8 of its `PlayerMaterialPresetParam` row, 0.1 to 4.5, 1.0 for the default 92) glued on as `%.1f`:
"Light Source Intensity1.5". A hook on that text function (`0x140f35c40(info, out)`; slot u8 at +0, material at
+0x20) returns "Brightness 150%" for slot 5's materials instead, in the picker and the panel. Editing text 210313
would only fix the spacing, and means shipping the menu message files.

The panel is built here rather than by the game's builder because that one adds Paint Pattern's Apply entry on an
AC with a pattern. It sits on the stock Color / Material clip, so nothing may be hidden on it: the stock entries
never show a hidden child again (a hidden material icon stayed hidden in every part's panel).

## Notes

Dropped or parked:
- Rows as made-up color slots (9-11) built by the game's own loop: hooks on the slot list (only inside the builder:
  other code fills six-entry arrays from it), is-normal-slot, the slot label, the edited slots, the paint color reader
  and writer (a seventh color would land in the pattern bytes), and the rows moved after WEATHERING by copy and
  destroy. Replaced by our own items, which need none of that.
- Choices as materials: a row's choices were materials added to the regulation (from ID 200 per row), listed by the
  stock material menu through hooks on the first material, the material reader and writer and both material text
  functions, and the panel came from the game's builder for the row's made-up slot. Replaced by the panel's own
  entries, which need none of that.
- Per-part rows (idea, nothing needs it): a row keeping one color per part section (10: head, core, arms, legs, hand
  weapons, back units, extension) instead of one for the AC. It fits in the row's own section, which has exactly ten
  color fields (6 colors + 4 pattern colors); the swatch and panel would read and write the selected category's.
  Not checked: whether the game normalises the pattern color fields of the free sections on load. Not for
  thrusters: flames belong to the booster and sit on core and legs only, and the type hook gives the AC.

Other findings:
- Paint writers next to the color one (`0x14045afc0`, +8): `0x14045aee0` pattern design (+0x2c), `0x14045af90`
  pattern size (+0x2d), `0x14045af30` pattern color (+0x30), `0x14045b020` weathering (+4), `0x14045b070` material
  (+0x20 + slot * 2; reached from `0x140e0d100`).
