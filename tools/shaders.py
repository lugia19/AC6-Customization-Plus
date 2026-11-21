"""The shader bundle with our keys on the weapons' color params (docs/material-overrides.md). Slow to repack: cached
until the game's file, the keys or FORMAT change."""
import json
import os
import re
import shutil
import struct
import zlib

from tools.witchy import unpacked

BUNDLE = os.path.join("shader_d3d12", "shaderbdle.shaderbdlebnd.dcx")
# Raise when the patching changes, so the cached bundle is rebuilt.
FORMAT = 1


def key(name):
    """An ex-param key: the CRC-32 of the name's UTF-16LE bytes (the game's hash, 0x14263c0a0)."""
    return zlib.crc32(name.encode("utf-16-le"))


def patch_shader(path, keys):
    """Sets the keys ({param name: key}) of a .shaderbdle's params, in place: in its .metaparam, each param's record
    points at its name and holds the key at +0x30."""
    data = bytearray(open(path, "rb").read())
    count = struct.unpack_from("<i", data, 0xC)[0]
    entry_size = struct.unpack_from("<q", data, 0x20)[0]
    done = set()
    for i in range(count):
        entry = 0x40 + i * entry_size
        size = struct.unpack_from("<q", data, entry + 8)[0]
        start = struct.unpack_from("<I", data, entry + 0x18)[0]
        name_at = struct.unpack_from("<I", data, entry + 0x20)[0]
        name_end = name_at
        while data[name_end:name_end + 2] != b"\0\0":
            name_end += 2
        if not data[name_at:name_end].decode("utf-16-le").endswith(".metaparam"):
            continue
        meta = data[start:start + size]
        for found in re.finditer(rb"(?:[\x20-\x7e]\x00){3,}", meta):
            param = found.group().decode("utf-16-le")
            if param not in keys:
                continue
            records = [m.start() for m in re.finditer(re.escape(struct.pack("<Q", found.start())), meta)
                       if m.start() % 8 == 0]
            assert len(records) == 1, f"{path}: {param} has {len(records)} records"
            struct.pack_into("<I", data, start + records[0] + 0x30, keys[param])
            done.add(param)
    assert done == set(keys), f"{path}: no records for {sorted(set(keys) - done)}"
    open(path, "wb").write(data)


def patched_bundle(game, cache, shader_keys, witchy):
    """The game's shader bundle with `shader_keys` ({shader: {param name: key}}) set, from the cache if it's current."""
    source = os.path.join(game, BUNDLE)
    stamp = {"format": FORMAT, "size": os.path.getsize(source), "mtime": os.path.getmtime(source), "keys": shader_keys}
    bundle = os.path.join(cache, os.path.basename(BUNDLE))
    stamp_path = bundle + ".json"
    if os.path.exists(bundle) and os.path.exists(stamp_path) and json.load(open(stamp_path)) == stamp:
        return bundle
    os.makedirs(cache, exist_ok=True)
    shutil.copy(source, bundle)
    witchy([bundle], [unpacked(bundle)])
    for shader, keys in shader_keys.items():
        patch_shader(os.path.join(unpacked(bundle), shader, shader + ".shaderbdle"), keys)
    witchy([unpacked(bundle)], [bundle])
    shutil.rmtree(unpacked(bundle))  # 660 MB
    json.dump(stamp, open(stamp_path, "w"))
    return bundle
