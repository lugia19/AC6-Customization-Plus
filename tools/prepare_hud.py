"""Prepares the HUD movie (menu/01_000_fe.gfx, a Scaleform GFX file: SWF tags) for the DLL's HUD rows, so the DLL
never has to read the file format (dll/src/hud_movie.rs):

- every top-level placement without a color transform gets a neutral one, whose alpha the DLL sets for the opacity;
- a table of every color in the movie is appended after it (the movie's own length doesn't count it, so the game
  never reads it): where the color is and how it is stored.

usage: prepare_hud.py <stock 01_000_fe.gfx> <output>

Table: entries of 8 bytes {u32 bit offset, u8 kind, u8 field width, i16 multiply}, then a footer {u32 entry count,
u32 offset of the table, "PMHUDTBL"}. Kinds:
  0 color bytes   red, green, blue bytes at the offset (fills, outlines, text)
  1 tint          a color transform's multiply terms (256 = 1.0): the multiply is the color
  2 mix           a color transform's add terms, mixing what's below (share: its grey multiply) with a color
  3 alpha         the alpha multiply of a transform added here
  4 hex text      six hex digits of an HTML text's color="#rrggbb" (byte offset * 8)
"""
import re
import struct
import sys

END, SHAPE, SHAPE2, PLACE_OBJECT2, SHAPE3, EDIT_TEXT, SPRITE, PLACE_OBJECT3, SHAPE4 = 0, 2, 22, 26, 32, 37, 39, 70, 83
HAS_CHARACTER, HAS_MATRIX, HAS_COLOR_TRANSFORM = 0x02, 0x04, 0x08
PLACE3_CLASS_NAME_OR_IMAGE = 0x18  # PlaceObject3's second flags byte: a class name follows the depth

COLOR_BYTES, TINT, MIX, ALPHA, HEX_TEXT = 0, 1, 2, 3, 4
MAGIC = b"PMHUDTBL"
HTML_COLOR = re.compile(rb'color="#([0-9a-fA-F]{6})"')
# CXFORMWITHALPHA, multiply terms only, 10-bit fields, all 256 (1.0): no add, multiply, width 10, then four terms.
NEUTRAL_TRANSFORM_BITS = "0" + "1" + "1010" + format(256, "010b") * 4
NEUTRAL_TRANSFORM = int(NEUTRAL_TRANSFORM_BITS.ljust(48, "0"), 2).to_bytes(6, "big")
ALPHA_BIT, ALPHA_WIDTH = 6 + 3 * 10, 10


def tags(d, start, end):
    """(type, body offset, body length, tag offset) of each tag in d[start:end]. Header: type in the top 10 bits,
    length in the low 6; 0x3f = a u32 length follows."""
    pos = start
    while pos < end:
        code = struct.unpack_from("<H", d, pos)[0]
        tag_type, length, body = code >> 6, code & 0x3f, pos + 2
        if length == 0x3f:
            length, body = struct.unpack_from("<I", d, pos + 2)[0], pos + 6
        yield tag_type, body, length, pos
        if tag_type == END:  # the file can have padding after it
            return
        pos = body + length


def tag(tag_type, body):
    if len(body) < 0x3f:
        return struct.pack("<H", (tag_type << 6) | len(body)) + body
    return struct.pack("<HI", (tag_type << 6) | 0x3f, len(body)) + body


def bits(d, pos, count, signed=False):
    value = 0
    for i in range(pos, pos + count):
        value = (value << 1) | ((d[i // 8] >> (7 - i % 8)) & 1)
    return value - (1 << count) if signed and count and value >> (count - 1) else value


def rect_end(d, pos):
    """End of a RECT record: a 5-bit field width and four fields."""
    return pos + (5 + 4 * bits(d, pos * 8, 5) + 7) // 8


def matrix_end(d, pos):
    """End of a MATRIX record: optional scale and rotate pairs, then the translation, each a 5-bit field width and
    two fields."""
    bit = pos * 8
    for _ in range(2):
        if bits(d, bit, 1):
            bit += 6 + 2 * bits(d, bit + 1, 5)
        else:
            bit += 1
    bit += 5 + 2 * bits(d, bit, 5)
    return (bit + 7) // 8


def placement_transform(d, tag_type, body):
    """Offset of a placement's color transform (after its flags, depth, character and matrix); None for a
    PlaceObject3 with a class name."""
    flags = d[body]
    pos = body + 1
    if tag_type == PLACE_OBJECT3:
        if d[pos] & PLACE3_CLASS_NAME_OR_IMAGE:
            return None
        pos += 1
    pos += 2  # depth
    if flags & HAS_CHARACTER:
        pos += 2
    if flags & HAS_MATRIX:
        pos = matrix_end(d, pos)
    return pos


def first_tag(d):
    """After the signature and version, the file length (u32 at 4), the stage RECT and the frame rate and count."""
    return rect_end(d, 8) + 4


def with_neutral_transforms(stock):
    """The movie with a neutral color transform on every top-level placement that has none (in the stock movie only
    an invisible layout helper has one), and the bit offsets of their alpha terms."""
    start = first_tag(stock)
    out = bytearray(stock[:start])
    alphas = []
    end = start
    for tag_type, body, length, pos in tags(stock, start, len(stock)):
        end = body + length
        flags = stock[body] if length else 0
        at = None
        if tag_type in (PLACE_OBJECT2, PLACE_OBJECT3) and flags & HAS_CHARACTER and not flags & HAS_COLOR_TRANSFORM:
            at = placement_transform(stock, tag_type, body)
        if at is None:
            out += stock[pos:end]
            continue
        new_body = bytes([flags | HAS_COLOR_TRANSFORM]) + stock[body + 1:at] + NEUTRAL_TRANSFORM + stock[at:end]
        header_size = len(tag(tag_type, new_body)) - len(new_body)
        alphas.append((len(out) + header_size + at - body) * 8 + ALPHA_BIT)
        out += tag(tag_type, new_body)
    out += stock[end:]
    struct.pack_into("<I", out, 4, len(out))
    return out, alphas


class Colors:
    """Finds every color in a movie: entries (bit offset, kind, field width, multiply)."""

    def __init__(self, d):
        self.d = d
        self.entries = []

    def color_bytes(self, pos):
        self.entries.append((pos * 8, COLOR_BYTES, 0, 0))

    def color_transform(self, pos):
        """CXFORMWITHALPHA: has-add and has-multiply bits, a 4-bit field width, then the multiply terms and the add
        terms, red green blue alpha each."""
        d, bit = self.d, pos * 8
        has_add, has_multiply, width = bits(d, bit, 1), bits(d, bit + 1, 1), bits(d, bit + 2, 4)
        multiply_at = bit + 6
        add_at = multiply_at + (4 * width if has_multiply else 0)
        multiply = [bits(d, multiply_at + i * width, width, True) for i in range(3)] if has_multiply else [256] * 3
        add = [bits(d, add_at + i * width, width, True) for i in range(3)] if has_add else [0] * 3
        grey_multiply = multiply[0] == multiply[1] == multiply[2]
        if add == [0, 0, 0]:
            if not grey_multiply:
                self.entries.append((multiply_at, TINT, width, 0))
        elif grey_multiply and min(add) >= 0:
            self.entries.append((add_at, MIX, width, multiply[0]))
        # A tint with add terms is left alone: what color it stands for depends on what it is applied to.

    def fill_style(self, pos, color_size):
        """One FILLSTYLE; returns its end."""
        kind = self.d[pos]
        pos += 1
        if kind == 0:
            self.color_bytes(pos)
            return pos + color_size
        if kind in (0x10, 0x12, 0x13):  # gradients: a matrix, a count of {ratio, color}; focal ones have 2 more bytes
            pos = matrix_end(self.d, pos)
            count = self.d[pos] & 0xf
            pos += 1
            for _ in range(count):
                self.color_bytes(pos + 1)
                pos += 1 + color_size
            return pos + 2 if kind == 0x13 else pos
        if kind in (0x40, 0x41, 0x42, 0x43):  # bitmaps: a character ID and a matrix
            return matrix_end(self.d, pos + 2)
        raise ValueError(f"fill style {kind:#x}")

    def style_count(self, pos):
        if self.d[pos] == 0xff:
            return struct.unpack_from("<H", self.d, pos + 1)[0], pos + 3
        return self.d[pos], pos + 1

    def style_arrays(self, tag_type, pos, color_size):
        """A FILLSTYLEARRAY and the LINESTYLEARRAY after it; returns their end."""
        count, pos = self.style_count(pos)
        for _ in range(count):
            pos = self.fill_style(pos, color_size)
        count, pos = self.style_count(pos)
        for _ in range(count):
            pos += 2  # width
            if tag_type == SHAPE4:
                # LINESTYLE2: two bytes of flags; a miter join has a limit; it has either a fill or a color.
                flags = self.d[pos]
                pos += 2
                if (flags >> 4) & 3 == 2:
                    pos += 2
                if flags & 0x08:
                    pos = self.fill_style(pos, color_size)
                    continue
            self.color_bytes(pos)
            pos += color_size
        return pos

    def shape(self, tag_type, body):
        """A DefineShape tag: its style arrays, and the ones inside its shape records."""
        d = self.d
        pos = rect_end(d, body + 2)
        if tag_type == SHAPE4:
            pos = rect_end(d, pos) + 1  # edge bounds, flags
        color_size = 4 if tag_type in (SHAPE3, SHAPE4) else 3
        pos = self.style_arrays(tag_type, pos, color_size)
        # After style arrays: a byte with the field widths of the fill and line style indices, then the records.
        fill_bits, line_bits = d[pos] >> 4, d[pos] & 0xf
        bit = (pos + 1) * 8
        while True:
            if bits(d, bit, 1):
                # An edge: straight flag, a 4-bit field width - 2, then the deltas.
                straight, width = bits(d, bit + 1, 1), bits(d, bit + 2, 4) + 2
                bit += 6
                if not straight:
                    bit += 4 * width
                elif bits(d, bit, 1):
                    bit += 1 + 2 * width  # general line
                else:
                    bit += 2 + width  # horizontal or vertical
                continue
            # A style change: new styles, line style, fill style 1, fill style 0, move to. All clear = the end.
            flags = bits(d, bit + 1, 5)
            bit += 6
            if flags == 0:
                return
            if flags & 1:
                bit += 5 + 2 * bits(d, bit, 5)
            if flags & 2:
                bit += fill_bits
            if flags & 4:
                bit += fill_bits
            if flags & 8:
                bit += line_bits
            if flags & 16:
                pos = self.style_arrays(tag_type, (bit + 7) // 8, color_size)
                fill_bits, line_bits = d[pos] >> 4, d[pos] & 0xf
                bit = (pos + 1) * 8

    def edit_text(self, body, length):
        """A DefineEditText tag's text color, and the colors in its HTML text: they win over the text color."""
        d = self.d
        pos = rect_end(d, body + 2)
        flags, flags2 = d[pos], d[pos + 1]
        pos += 2
        has_font, has_font_class, has_color = flags & 0x01, flags2 & 0x80, flags & 0x04
        if has_font:
            pos += 2
        if has_font_class:
            pos = d.index(b"\0", pos) + 1
        if has_font or has_font_class:
            pos += 2  # font height
        if has_color:
            self.color_bytes(pos)
            pos += 4
        if flags & 0x02:  # max length
            pos += 2
        if flags2 & 0x20:  # layout
            pos += 9
        pos = d.index(b"\0", pos) + 1  # variable name
        if flags & 0x80 and flags2 & 0x02:  # HTML text
            for found in HTML_COLOR.finditer(d, pos, body + length):
                self.entries.append((found.start(1) * 8, HEX_TEXT, 0, 0))

    def walk(self, start, end):
        for tag_type, body, length, _ in tags(self.d, start, end):
            if tag_type in (SHAPE, SHAPE2, SHAPE3, SHAPE4):
                self.shape(tag_type, body)
            elif tag_type == EDIT_TEXT:
                self.edit_text(body, length)
            elif tag_type == SPRITE:  # character ID, frame count, then its own tags
                self.walk(body + 4, body + length)
            elif tag_type in (PLACE_OBJECT2, PLACE_OBJECT3) and self.d[body] & HAS_COLOR_TRANSFORM:
                at = placement_transform(self.d, tag_type, body)
                if at is not None:
                    self.color_transform(at)


def prepare(stock):
    """The prepared movie with its table, and the entries."""
    movie, alphas = with_neutral_transforms(stock)
    colors = Colors(movie)
    colors.walk(first_tag(movie), len(movie))
    entries = colors.entries + [(bit, ALPHA, ALPHA_WIDTH, 0) for bit in alphas]
    table = b"".join(struct.pack("<IBBh", *entry) for entry in entries)
    return movie + table + struct.pack("<II", len(entries), len(movie)) + MAGIC, entries


def main():
    stock = open(sys.argv[1], "rb").read()
    prepared, entries = prepare(stock)
    open(sys.argv[2], "wb").write(prepared)
    counts = [sum(1 for e in entries if e[1] == kind) for kind in (COLOR_BYTES, TINT, MIX, ALPHA, HEX_TEXT)]
    print(f"HUD movie prepared: {counts[0]} color bytes, {counts[1]} tints, {counts[2]} mixes, "
          f"{counts[4]} HTML text colors, {counts[3]} top-level elements; table {len(prepared) - len(stock)} bytes")


if __name__ == "__main__":
    main()
