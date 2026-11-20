"""Raw .param files from an unpacked regulation, edited in place (fields read and written, rows appended)."""
import os
import re
import struct
import xml.etree.ElementTree as ET

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
