"""Raw .param files from an unpacked regulation, read through WitchyBND's Paramdex defs."""
import os
import re
import struct
import xml.etree.ElementTree as ET

FIELD_SIZES = {'s8': 1, 'u8': 1, 'dummy8': 1, 's16': 2, 'u16': 2, 's32': 4, 'u32': 4, 'f32': 4, 'angle32': 4,
               'b32': 4, 'f64': 8, 'fixstr': 1, 'fixstrW': 2}
FIELD_FORMATS = {'s8': '<b', 'u8': '<B', 's16': '<h', 'u16': '<H', 's32': '<i', 'u32': '<I'}


class Param:
    """Raw .param file, fields located through WitchyBND's Paramdex defs (or `layout`, a list of (type, name) for
    params without one)."""

    def __init__(self, path, def_path=None, layout=None):
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

    def read_rows(self):
        # The row table: row_count entries of 0x18 bytes from 0x40, each an s32 row ID, 4 bytes of padding, the u64
        # offset of the row's data and the u64 offset of its name.
        count = struct.unpack_from("<H", self.data, 0xA)[0]
        self.rows = dict(struct.unpack_from("<i4xQ", self.data, 0x40 + i * 0x18) for i in range(count))  # ID -> data
