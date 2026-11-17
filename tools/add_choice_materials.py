"""Adds the materials an extra Paint row's choices need to a regulation's PlayerMaterialPresetParam.param (the raw
file, as unpacked by WitchyBND): one row per choice, with consecutive IDs from the row's first material. Run it on
the regulation before repacking it (build.py does).

usage: add_choice_materials.py PlayerMaterialPresetParam.param <row 0-2> <number of choices>
"""
import struct
import sys

MAX_CHOICES = 36  # dll/src/rows.rs MAX_CHOICES


def first_material(row):
    """dll/src/rows.rs first_material: 200, 300, 400 (above the stock materials 0-35 and 90-99, with a gap after 99)."""
    return 200 + row * 100


def add_param_rows(path, copies):
    """Adds rows to a raw .param file: copies = {new row ID: ID of the row whose data it starts as}. The row table
    stays sorted by ID (the game finds rows by binary search); the new rows' data goes after the existing data."""
    data = bytearray(open(path, "rb").read())
    # Header: strings offset (u32 at 0), row count (u16 at 0xA), param type name offset (u64 at 0x10), data start
    # (u64 at 0x30). Then the row table from 0x40: s32 ID, 4 bytes of padding, u64 data offset, u64 name offset. Then
    # the rows' data, then the strings.
    strings_offset = struct.unpack_from("<I", data, 0)[0]
    row_count = struct.unpack_from("<H", data, 0xA)[0]
    rows = [struct.unpack_from("<i4xQQ", data, 0x40 + i * 0x18) for i in range(row_count)]
    data_offsets = sorted(data_offset for _, data_offset, _ in rows)
    row_sizes = {b - a for a, b in zip(data_offsets, data_offsets[1:])}
    assert len(row_sizes) == 1, f"rows of different sizes: {row_sizes}"
    row_size = row_sizes.pop()
    assert struct.unpack_from("<Q", data, 0x10)[0] == strings_offset, "param type name not at the strings offset"
    assert data_offsets[-1] + row_size == strings_offset, "row data doesn't end at the strings"
    existing = [row_id for row_id in copies if any(row_id == other for other, _, _ in rows)]
    assert not existing, f"rows {existing} already exist"

    # The table grows, which moves the data and the strings; the new data moves the strings again.
    table_growth = len(copies) * 0x18
    growth = table_growth + len(copies) * row_size
    row_data = {row_id: bytes(data[data_offset:data_offset + row_size]) for row_id, data_offset, _ in rows}
    name_offset = rows[0][2]  # every row has the same (empty) name
    table = [(row_id, data_offset + table_growth, row_name_offset + growth) for row_id, data_offset, row_name_offset in rows]
    new_data = b""
    for i, (new_id, source_id) in enumerate(sorted(copies.items())):
        table.append((new_id, strings_offset + table_growth + i * row_size, name_offset + growth))
        new_data += row_data[source_id]
    new_table = b"".join(struct.pack("<i4xQQ", *row) for row in sorted(table))

    struct.pack_into("<I", data, 0, strings_offset + growth)
    struct.pack_into("<H", data, 0xA, row_count + len(copies))
    struct.pack_into("<Q", data, 0x10, strings_offset + growth)
    struct.pack_into("<Q", data, 0x30, struct.unpack_from("<Q", data, 0x30)[0] + table_growth)
    data[strings_offset:strings_offset] = new_data
    data[0x40:0x40 + row_count * 0x18] = new_table
    open(path, "wb").write(data)


def main():
    path, row, count = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    assert 0 <= row <= 2 and 1 <= count <= MAX_CHOICES, f"row 0-2, 1-{MAX_CHOICES} choices"
    first = first_material(row)
    # The values aren't used (no part has these materials): copies of material 0.
    add_param_rows(path, {first + i: 0 for i in range(count)})
    print(f"materials {first}-{first + count - 1} added")


if __name__ == "__main__":
    main()
