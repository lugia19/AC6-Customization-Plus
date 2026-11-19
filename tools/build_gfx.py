"""Builds the Paint movie (02_coloring.gfx) from the stock one: three extra color rows (Item_9_0..Item_11_0) below
WEATHERING in the Paint list (dll/src/rows.rs's ROW_COUNT), and a panel for them. Everything else stays byte-identical.

usage: build_gfx.py stock.gfx out.gfx

The list's panel (sprite 268) is a fixed size: a solid rectangle (shape 129, scaled), and a texture (sprite 130)
masked by a rectangle (shape 217). The extra rows get a panel of their own below it, built the same way and darker:
each extra row is a copy of the color row sprite (244) with its slice of that panel underneath, so the panel is as
tall as the rows the game shows (clips past the row count are hidden). The copies are bound to the row sprite's
ActionScript class, whose frame scripts stop its animations (without it they loop). The first one also has the
panel's header, a text field copied from the rows' label field (228) with its own text.

The rows panel: the game looks its panels up by name in the content sprite (269) and fills a panel's Item_<n>_0 clips
with a panel's entries, in order. The Color / Material panel (192) has a color item, a material item and the Paint
Pattern item (fixed text "Apply"). The extra rows' panel is a copy of it with color, color, material and color items,
under its own name (dll/src/panel.rs builds the panel on it).
"""
import struct
import sys

LIST_SPRITE = 268
ROW_SPRITE = 244
PANEL_SHAPE = 129  # solid rectangle, bounds below
PANEL_SHAPE_BOUNDS = (1920, 10920, 17260, 19500)  # xmin, xmax, ymin, ymax
PANEL_TEXTURE_DEPTH = 3  # the texture's placement in the list sprite (character, matrix, color transform)
NEW_SPRITE_IDS = (398, 399, 400)  # stock's highest character ID is 397
ROW_LABEL_TEXT = 228  # the rows' label: a DefineEditText (font class, height, color, layout, initial text)
HEADER_TEXT_ID = 401
HEADER = "ENTIRE AC"
NEW_ROW_DEPTHS = (7, 8, 9)  # free depths in the list sprite
FIRST_NEW_ROW = 9  # Item_<n>_0
CONTENT_SPRITE = 269
STOCK_PANEL_SPRITE = 192
# The Color / Material panel's items: Item_0_0 color, Item_1_0 material, Item_2_0 Paint Pattern. The rows' panel
# places color, color, material, color (dll/src/panel.rs adds a row's color, second color, choices, third color in
# that order).
PANEL_COLOR_ITEM = 191
PANEL_MATERIAL_ITEM = 185
PANEL_PATTERN_ITEM = 178
ROWS_PANEL_ITEMS = {PANEL_MATERIAL_ITEM: PANEL_COLOR_ITEM, PANEL_PATTERN_ITEM: PANEL_MATERIAL_ITEM}
# The fourth item: free depth (the stock items use 10, 22, 36), position one item pitch (2280) below Item_2_0.
ROWS_PANEL_ITEM_3_DEPTH = 48
ROWS_PANEL_ITEM_3 = (10900, 8940 + 2280)
ROWS_PANEL_ID = 402
ROWS_PANEL_NAME = "Color_Material_Color_Select"
STOCK_PANEL_DEPTH = 118  # the Color / Material panel's in the content sprite
ROWS_PANEL_DEPTH = 119  # free, right above it

# Layout, in twips (20 per pixel at 1080p), list sprite coordinates.
PANEL_X = (1920, 8820)
PANEL_BOTTOM = 16480
PANEL_GAP = 160  # between the stock panel and the extra rows' panel
PANEL_MARGIN = 220  # panel below the last row
HEADER_Y = 100  # header below the panel's top
HEADER_X = 100  # header right of the rows' left edge
HEADER_FONT_HEIGHT = 300
HEADER_COLOR = bytes([0xdf, 0xfc, 0xff, 0xa0])  # the labels' color, fainter
HEADER_HEIGHT = 520  # panel above the first row: the header and the space around it
ROW_X = 2760
ROW_HEIGHT = 800
ROW_PITCH = 950  # the stock rows are 1000 apart; three rows and the header have to end above the key guide
# The extra panel's texture, darker than stock: red, green, blue, alpha multipliers out of 256 (stock: 256, 256, 256, 154).
PANEL_TINT = (176, 176, 176, 154)
SLICE_DEPTHS = 4  # depths the slice (and the header) take at the bottom of a row sprite

# The option lists' host: the Paint movie's own single-column list, Text_ItemList (sprite 397, placed at the root),
# has three item slots, and its background Base (sprite 389) one frame per item count up to 3 (the list sets frame
# min(count, frame count), 0x140c76730). Our lists get a copy with TALL_HOST_SLOTS slots: Base's frames 4.. stretch
# its rectangle (shape 127, 2000 twips square, scaled) and the frame-3 outline (sprite 388, made scale-9 so its
# lines keep their width) by one item pitch each.
STOCK_HOST, STOCK_HOST_BASE, STOCK_HOST_ITEM, STOCK_HOST_OUTLINE = 397, 389, 393, 388
STOCK_HOST_DEPTH = 2007  # its root placement; the root fades it in over frames 2-14
STOCK_OUTLINE_BOUNDS = (9920, 20020, 7780, 14540)  # shape 387, in sprite 388
SCALE9_INSET = 200  # the outline's corners and edges, kept unscaled
BASE_RECT = (10035, 7919, 4.939, 3.25)  # frame 3's rectangle: x, y, scale x, scale y (of the 2000-twip square)
BASE_RECT_UNIT = 2000
HOST_ITEM_X, HOST_ITEM_Y, HOST_ITEM_PITCH = 11760, 9840, 1000
TALL_HOST_SLOTS = 11
# The stock host's box grows down from mid-screen; ours is placed this much higher so 11 rows end where 8 would.
TALL_HOST_Y = -3000
TALL_HOST_ID, TALL_BASE_ID, TALL_OUTLINE_ID = 403, 404, 405
TALL_HOST_NAME = "Text_ItemList_Tall"  # dll/src/panel.rs opens its option lists there
TALL_HOST_DEPTH = 2100  # free at the root
TALL_ITEM_DEPTHS = range(2, 2 + TALL_HOST_SLOTS - 3)  # free in the host sprite (Base 1, stock items 10-22)

SHOW_FRAME, PLACE_OBJECT2, REMOVE_OBJECT2, DEFINE_EDIT_TEXT, DEFINE_SPRITE, FRAME_LABEL, SYMBOL_CLASS, END = \
    1, 26, 28, 37, 39, 43, 76, 0
DEFINE_SCALING_GRID = 78


# ---- SWF records ---------------------------------------------------------------------------------------------------

def pack_bits(bit_string):
    bit_string += "0" * (-len(bit_string) % 8)
    return bytes(int(bit_string[i:i + 8], 2) for i in range(0, len(bit_string), 8))


def signed_bits(values):
    """The values as signed fields of the smallest common width: (width, concatenated bits)."""
    width = max(v.bit_length() + 1 if v >= 0 else (~v).bit_length() + 1 for v in values)
    return width, "".join(format(v & ((1 << width) - 1), f"0{width}b") for v in values)


def matrix(tx, ty, scale=None):
    """MATRIX record: a translation, and optionally an (x, y) scale."""
    out = "0"
    if scale:
        width, fields = signed_bits([round(s * 65536) for s in scale])  # 16.16 fixed point
        out = "1" + format(width, "05b") + fields
    width, fields = signed_bits([tx, ty])
    return pack_bits(out + "0" + format(width, "05b") + fields)


def read_matrix(body, pos):
    """(byte length, tx, ty) of the MATRIX record at body[pos]."""
    stream = "".join(format(b, "08b") for b in body[pos:pos + 40])
    i = 0
    for _ in range(2):  # scale, rotate
        if stream[i] == "1":
            i += 6 + 2 * int(stream[i + 1:i + 6], 2)
        else:
            i += 1
    width = int(stream[i:i + 5], 2)
    tx, ty = signed_fields(stream, i + 5, width, 2)
    return (i + 5 + 2 * width + 7) // 8, tx, ty


def signed_fields(stream, start, width, count):
    """`count` signed fields of `width` bits from a string of bits."""
    values = [int(stream[start + n * width:start + (n + 1) * width] or "0", 2) for n in range(count)]
    return [v - (1 << width) if width and v >> (width - 1) else v for v in values]


def read_rect(body, pos):
    """(byte length, [xmin, xmax, ymin, ymax]) of the RECT record at body[pos]: a 5-bit field width, then the fields."""
    width = body[pos] >> 3
    size = (5 + 4 * width + 7) // 8
    stream = "".join(format(b, "08b") for b in body[pos:pos + size])
    return size, signed_fields(stream, 5, width, 4)


def color_multiply(terms):
    """CXFORMWITHALPHA record with multiply terms only."""
    width, fields = signed_bits(terms)
    return pack_bits("01" + format(width, "04b") + fields)


def tag(tag_type, body):
    if len(body) < 0x3f:
        return struct.pack("<H", (tag_type << 6) | len(body)) + body
    return struct.pack("<HI", (tag_type << 6) | 0x3f, len(body)) + body


def tags(d, start, end):
    """(type, header size, offset, raw bytes) of each tag in d[start:end]."""
    pos = start
    while pos < end:
        code = struct.unpack_from("<H", d, pos)[0]
        tag_type, length, header = code >> 6, code & 0x3f, 2
        if length == 0x3f:
            length, header = struct.unpack_from("<I", d, pos + 2)[0], 6
        yield tag_type, header, pos, bytes(d[pos:pos + header + length])
        if tag_type == END:  # the file can have padding after it
            break
        pos += header + length


def first_tag_pos(d):
    """Offset of the first tag: after the 8-byte header, the stage RECT (variable size) and frame rate/count."""
    return 8 + read_rect(d, 8)[0] + 4


def find_tag(d, tag_type, character_id):
    """(offset, header size, raw bytes) of the root-level tag of that type defining that character."""
    for t, header, pos, raw in tags(d, first_tag_pos(d), len(d)):
        if t == tag_type and struct.unpack_from("<H", raw, header)[0] == character_id:
            return pos, header, raw
    sys.exit(f"tag {tag_type} for character {character_id} not found")


def find_sprite(d, sprite_id):
    """(offset, header size, body length) of a DefineSprite tag."""
    pos, header, raw = find_tag(d, DEFINE_SPRITE, sprite_id)
    return pos, header, len(raw) - header


def shift_depth(body, by):
    """A PlaceObject2 body with its depth (and clip depth) raised."""
    body = bytearray(body)
    flags = body[0]
    struct.pack_into("<H", body, 1, struct.unpack_from("<H", body, 1)[0] + by)
    if flags & 0x40:
        assert not flags & 0x98, "clip depth after a color transform, ratio or clip actions: not handled"
        p = 3 + (2 if flags & 0x02 else 0)
        if flags & 0x04:
            p += read_matrix(body, p)[0]
        if flags & 0x20:
            p = body.index(b"\0", p) + 1
        struct.pack_into("<H", body, p, struct.unpack_from("<H", body, p)[0] + by)
    return bytes(body)


def symbol_classes(body):
    """A SymbolClass tag's entries: character ID -> class name (bytes)."""
    classes, pos = {}, 2
    for _ in range(struct.unpack_from("<H", body)[0]):
        end = body.index(b"\0", pos + 2)
        classes[struct.unpack_from("<H", body, pos)[0]] = body[pos + 2:end]
        pos = end + 1
    return classes


# ---- The extra rows ------------------------------------------------------------------------------------------------

def panel_rect_matrix(x0, x1, y0, y1):
    """Matrix that puts the panel shape on a rectangle."""
    bx0, bx1, by0, by1 = PANEL_SHAPE_BOUNDS
    sx, sy = (x1 - x0) / (bx1 - bx0), (y1 - y0) / (by1 - by0)
    return matrix(round(x0 - bx0 * sx), round(y0 - by0 * sy), scale=(sx, sy))


def header_text(label_text):
    """The header's DefineEditText tag, from the rows' label's (its content): its font class, with our font height,
    color and text."""
    pos = 2 + read_rect(label_text, 2)[0]
    flags = label_text[pos:pos + 2]
    # HasText, ReadOnly, HasTextColor / HasFontClass, HasLayout, UseOutlines: the fields below, in this order.
    assert flags == bytes([0x8c, 0xa1]), "label text field: unexpected flags"
    font_class_end = label_text.index(b"\0", pos + 2) + 1
    layout = label_text[font_class_end + 6:font_class_end + 15]  # after the font height (u16) and color (RGBA)
    assert label_text[font_class_end + 15] == 0, "label text field: has a variable name"
    return tag(DEFINE_EDIT_TEXT, struct.pack("<H", HEADER_TEXT_ID) + label_text[2:font_class_end]
               + struct.pack("<H", HEADER_FONT_HEIGHT) + HEADER_COLOR + layout + b"\0" + HEADER.encode() + b"\0")


def row_sprite(stock_row, new_id, row_y, slice_top, texture, header):
    """The color row sprite (its tag's content) with a panel slice from slice_top to the margin below the row, under
    its content, and the header at the slice's top if asked. texture: the panel texture's character ID bytes and
    position in the list."""
    texture_char, texture_x, texture_y = texture
    rect = panel_rect_matrix(PANEL_X[0] - ROW_X, PANEL_X[1] - ROW_X, slice_top - row_y, ROW_HEIGHT + PANEL_MARGIN)
    # Rectangle, the same rectangle as the mask of the texture, the texture where it lines up with the stock panel's.
    body = tag(PLACE_OBJECT2, bytes([0x06]) + struct.pack("<HH", 1, PANEL_SHAPE) + rect)
    body += tag(PLACE_OBJECT2, bytes([0x46]) + struct.pack("<HH", 2, PANEL_SHAPE) + rect + struct.pack("<H", 3))
    body += tag(PLACE_OBJECT2, bytes([0x0e]) + struct.pack("<H", 3) + texture_char
                + matrix(texture_x - ROW_X, texture_y - row_y) + color_multiply(PANEL_TINT))
    if header:
        body += tag(PLACE_OBJECT2, bytes([0x06]) + struct.pack("<HH", 4, HEADER_TEXT_ID)
                    + matrix(HEADER_X, slice_top - row_y + HEADER_Y))
    frame_count = stock_row[2:4]
    for tag_type, header, _, raw in tags(stock_row, 4, len(stock_row)):
        if tag_type == PLACE_OBJECT2:
            raw = tag(PLACE_OBJECT2, shift_depth(raw[header:], SLICE_DEPTHS))
        else:
            assert tag_type in (END, SHOW_FRAME, FRAME_LABEL), f"row sprite: unexpected tag {tag_type}"
        body += raw
    return tag(DEFINE_SPRITE, struct.pack("<H", new_id) + frame_count + body)


def rows_panel_sprite(stock_panel):
    """The Color / Material panel sprite (its tag's content) with its items replaced as ROWS_PANEL_ITEMS says, and a
    fourth item (a color) below them. Its background already reaches past it (sized for the Paint Pattern preview)."""
    body, swapped = b"", 0
    for tag_type, header, _, raw in tags(stock_panel, 4, len(stock_panel)):
        place = raw[header:]
        if tag_type == PLACE_OBJECT2 and place[0] & 0x02 and struct.unpack_from("<H", place, 3)[0] in ROWS_PANEL_ITEMS:
            new = ROWS_PANEL_ITEMS[struct.unpack_from("<H", place, 3)[0]]
            raw = tag(PLACE_OBJECT2, place[:3] + struct.pack("<H", new) + place[5:])
            swapped += 1
        if tag_type == SHOW_FRAME:
            body += tag(PLACE_OBJECT2, bytes([0x26]) + struct.pack("<HH", ROWS_PANEL_ITEM_3_DEPTH, PANEL_COLOR_ITEM)
                        + matrix(*ROWS_PANEL_ITEM_3) + b"Item_3_0\0")
        body += raw
    assert swapped == len(ROWS_PANEL_ITEMS), "rows panel sprite: an item to replace isn't placed exactly once"
    return tag(DEFINE_SPRITE, struct.pack("<H", ROWS_PANEL_ID) + stock_panel[2:4] + body)


def add_rows_panel(d):
    """Adds the extra rows' panel to the content sprite. Returns the bytes added."""
    stock_pos, stock_header, stock_length = find_sprite(d, STOCK_PANEL_SPRITE)
    sprite = rows_panel_sprite(bytes(d[stock_pos + stock_header:stock_pos + stock_header + stock_length]))
    content_pos, content_header, content_length = find_sprite(d, CONTENT_SPRITE)
    assert content_header == 6, "content sprite must use the long length form"
    content_tags = list(tags(d, content_pos + content_header + 4, content_pos + content_header + content_length))
    assert all(t in (PLACE_OBJECT2, SHOW_FRAME, END) for t, _, _, _ in content_tags), "content sprite: unexpected tag"
    used_depths = {struct.unpack_from("<H", raw, h + 1)[0] for t, h, _, raw in content_tags if t == PLACE_OBJECT2}
    assert ROWS_PANEL_DEPTH not in used_depths, "the rows panel's depth is in use in the content sprite"
    # The content sprite places its panels in frame 1 and fades them over the next frames (a color transform per
    # panel per frame): ours gets the Color / Material panel's tags again, at its own depth. Placed visible like the
    # others: the timeline replays (e.g. coming back from the color picker), and a placement that says "invisible"
    # would hide the panel again each time. dll/src/panel.rs hides it at screen setup, as the game does its own.
    added = 0
    for tag_type, header, pos, raw in reversed(content_tags):  # back to front: earlier offsets stay valid
        if tag_type != PLACE_OBJECT2 or struct.unpack_from("<H", raw, header + 1)[0] != STOCK_PANEL_DEPTH:
            continue
        place = bytearray(raw[header:])
        struct.pack_into("<H", place, 1, ROWS_PANEL_DEPTH)
        if place[0] & 0x02:  # the placement itself: our sprite, our name
            assert place[0] == 0x26, "content sprite: the Color / Material panel's placement has unexpected fields"
            struct.pack_into("<H", place, 3, ROWS_PANEL_ID)
            name_start = 5 + read_matrix(place, 5)[0]
            place = place[:name_start] + ROWS_PANEL_NAME.encode() + b"\0"
        new = tag(PLACE_OBJECT2, bytes(place))
        d[pos + len(raw):pos + len(raw)] = new
        added += len(new)
    assert added, "content sprite: the Color / Material panel isn't placed"
    struct.pack_into("<I", d, content_pos + 2, content_length + added)
    d[content_pos:content_pos] = sprite
    return len(sprite) + added


# ---- The tall option list host -------------------------------------------------------------------------------------

def sprite_body(d, sprite_id):
    """A DefineSprite's content: frame count (u16) and tags, without the sprite ID."""
    pos, header, length = find_sprite(d, sprite_id)
    return bytes(d[pos + header + 2:pos + header + length])


def before_end(body, extra):
    """A sprite content with tags added before its End tag."""
    assert body.endswith(tag(END, b"")), "sprite content must end with End"
    return body[:-2] + extra + tag(END, b"")


def tall_base(stock_base):
    """Base with frames 4..TALL_HOST_SLOTS: each one item pitch taller than the last."""
    x, y, scale_x, scale_y = BASE_RECT
    x0, x1, y0, y1 = STOCK_OUTLINE_BOUNDS
    frames = b""
    for extra in range(1, TALL_HOST_SLOTS - 2):
        rect = matrix(x, y, scale=(scale_x, scale_y + extra * HOST_ITEM_PITCH / BASE_RECT_UNIT))
        stretch = (y1 - y0 + extra * HOST_ITEM_PITCH) / (y1 - y0)
        outline = matrix(0, round(y0 * (1 - stretch)), scale=(1, stretch))
        frames += (tag(PLACE_OBJECT2, bytes([0x05]) + struct.pack("<H", 1) + rect)
                   + tag(REMOVE_OBJECT2, struct.pack("<H", 8))
                   + tag(PLACE_OBJECT2, bytes([0x06]) + struct.pack("<HH", 8, TALL_OUTLINE_ID) + outline)
                   + tag(SHOW_FRAME, b""))
    assert struct.unpack_from("<H", stock_base)[0] == 3, "Base: expected 3 frames"
    return struct.pack("<H", TALL_HOST_SLOTS) + before_end(stock_base[2:], frames)


def tall_host(stock_host):
    """Text_ItemList on the tall Base, with items 3..TALL_HOST_SLOTS-1 below the stock three."""
    body, rebased = b"", 0
    for tag_type, header, _, raw in tags(stock_host, 2, len(stock_host)):
        place = raw[header:]
        if tag_type == PLACE_OBJECT2 and place[0] & 0x02 and struct.unpack_from("<H", place, 3)[0] == STOCK_HOST_BASE:
            raw = tag(PLACE_OBJECT2, place[:3] + struct.pack("<H", TALL_BASE_ID) + place[5:])
            rebased += 1
        elif tag_type == PLACE_OBJECT2:
            assert struct.unpack_from("<H", place, 1)[0] not in TALL_ITEM_DEPTHS, "host: an item depth is in use"
        if tag_type == SHOW_FRAME and rebased == 1:
            for depth, item in zip(TALL_ITEM_DEPTHS, range(3, TALL_HOST_SLOTS)):
                body += tag(PLACE_OBJECT2, bytes([0x26]) + struct.pack("<HH", depth, STOCK_HOST_ITEM)
                            + matrix(HOST_ITEM_X, HOST_ITEM_Y + item * HOST_ITEM_PITCH) + f"Item_{item}_0".encode() + b"\0")
            rebased += 1
        body += raw
    assert rebased == 2, "host: Base isn't placed exactly once in frame 1"
    return stock_host[:2] + body


def add_tall_host(d):
    """Adds the tall host's characters before the stock host, and mirrors the stock host's root placement and moves
    at TALL_HOST_DEPTH. Returns the bytes added."""
    x0, x1, y0, y1 = STOCK_OUTLINE_BOUNDS
    width, fields = signed_bits([x0 + SCALE9_INSET, x1 - SCALE9_INSET, y0 + SCALE9_INSET, y1 - SCALE9_INSET])
    characters = (tag(DEFINE_SPRITE, struct.pack("<H", TALL_OUTLINE_ID) + sprite_body(d, STOCK_HOST_OUTLINE))
                  + tag(DEFINE_SCALING_GRID, struct.pack("<H", TALL_OUTLINE_ID) + pack_bits(format(width, "05b") + fields))
                  + tag(DEFINE_SPRITE, struct.pack("<H", TALL_BASE_ID) + tall_base(sprite_body(d, STOCK_HOST_BASE)))
                  + tag(DEFINE_SPRITE, struct.pack("<H", TALL_HOST_ID) + tall_host(sprite_body(d, STOCK_HOST))))
    root_tags = list(tags(d, first_tag_pos(d), len(d)))
    assert not any(t == PLACE_OBJECT2 and struct.unpack_from("<H", raw, h + 1)[0] == TALL_HOST_DEPTH
                   for t, h, _, raw in root_tags), "the tall host's root depth is in use"
    added = 0
    for tag_type, header, pos, raw in reversed(root_tags):  # back to front: earlier offsets stay valid
        if tag_type != PLACE_OBJECT2 or struct.unpack_from("<H", raw, header + 1)[0] != STOCK_HOST_DEPTH:
            continue
        place = bytearray(raw[header:])
        struct.pack_into("<H", place, 1, TALL_HOST_DEPTH)
        if place[0] & 0x02:  # the placement itself: our sprite, our name
            assert place[0] == 0x26, "root: the stock host's placement has unexpected fields"
            struct.pack_into("<H", place, 3, TALL_HOST_ID)
            _, x, y = read_matrix(place, 5)
            assert (x, y) == (0, 0), "root: the stock host isn't placed at 0, 0"
            place = place[:5] + matrix(0, TALL_HOST_Y) + TALL_HOST_NAME.encode() + b"\0"
        new = tag(PLACE_OBJECT2, bytes(place))
        d[pos + len(raw):pos + len(raw)] = new
        added += len(new)
    host_pos = find_tag(d, DEFINE_SPRITE, STOCK_HOST)[0]
    d[host_pos:host_pos] = characters
    return added + len(characters)


def main():
    inp, out = sys.argv[1], sys.argv[2]
    d = bytearray(open(inp, "rb").read())
    used = {struct.unpack_from("<H", raw, h)[0] for _, h, _, raw in tags(d, first_tag_pos(d), len(d)) if len(raw) - h >= 2}
    assert not used & {*NEW_SPRITE_IDS, HEADER_TEXT_ID, ROWS_PANEL_ID, TALL_HOST_ID, TALL_BASE_ID, TALL_OUTLINE_ID}, \
        "a new character's ID may be in use"
    _, label_header, label_raw = find_tag(d, DEFINE_EDIT_TEXT, ROW_LABEL_TEXT)
    label_text = label_raw[label_header:]
    row_pos, row_header, row_length = find_sprite(d, ROW_SPRITE)
    stock_row = bytes(d[row_pos + row_header:row_pos + row_header + row_length])
    list_pos, list_header, list_length = find_sprite(d, LIST_SPRITE)
    assert list_header == 6, "list sprite must use the long length form"
    list_tags = list(tags(d, list_pos + list_header + 4, list_pos + list_header + list_length))

    # The panel texture's placement in the list: flags, depth, character, matrix (translation only), color transform.
    texture_place = next(raw[h:] for t, h, _, raw in list_tags if t == PLACE_OBJECT2 and raw[h] == 0x0e
                         and struct.unpack_from("<H", raw, h + 1)[0] == PANEL_TEXTURE_DEPTH)
    texture = (texture_place[3:5], *read_matrix(texture_place, 5)[1:])
    used_depths = {struct.unpack_from("<H", raw, h + 1)[0] for t, h, _, raw in list_tags if t == PLACE_OBJECT2}
    assert not used_depths & set(NEW_ROW_DEPTHS), "a new row's depth is in use in the list sprite"

    sprites, places = header_text(label_text), b""
    slice_top = PANEL_BOTTOM + PANEL_GAP
    for i, (new_id, depth) in enumerate(zip(NEW_SPRITE_IDS, NEW_ROW_DEPTHS)):
        row_y = PANEL_BOTTOM + PANEL_GAP + HEADER_HEIGHT + i * ROW_PITCH
        sprites += row_sprite(stock_row, new_id, row_y, slice_top, texture, header=i == 0)
        name = f"Item_{FIRST_NEW_ROW + i}_0".encode() + b"\0"
        places += tag(PLACE_OBJECT2, bytes([0x26]) + struct.pack("<HH", depth, new_id) + matrix(ROW_X, row_y) + name)
        slice_top = row_y + ROW_HEIGHT + PANEL_MARGIN

    # The new sprites' class, added to the SymbolClass tag (after the sprites in the file, so edited first).
    class_pos, class_header, class_raw = next((pos, h, raw) for t, h, pos, raw in tags(d, list_pos, len(d)) if t == SYMBOL_CLASS)
    class_body = class_raw[class_header:]
    row_class = symbol_classes(class_body)[ROW_SPRITE]
    entries = b"".join(struct.pack("<H", new_id) + row_class + b"\0" for new_id in NEW_SPRITE_IDS)
    count = struct.unpack_from("<H", class_body)[0] + len(NEW_SPRITE_IDS)
    d[class_pos:class_pos + len(class_raw)] = tag(SYMBOL_CLASS, struct.pack("<H", count) + class_body[2:] + entries)

    # Placements go before the list's first ShowFrame, the new sprites before the list sprite.
    first_frame = next(pos for t, _, pos, _ in list_tags if t == SHOW_FRAME)
    d[first_frame:first_frame] = places
    struct.pack_into("<I", d, list_pos + 2, list_length + len(places))
    d[list_pos:list_pos] = sprites
    added = len(sprites) + len(places) + len(entries) + add_rows_panel(d) + add_tall_host(d)
    struct.pack_into("<I", d, 4, struct.unpack_from("<I", d, 4)[0] + added)
    open(out, "wb").write(d)
    print(f"{len(NEW_SPRITE_IDS)} rows, their panel and a {TALL_HOST_SLOTS}-item list added, size {added:+d} bytes")


if __name__ == "__main__":
    main()
