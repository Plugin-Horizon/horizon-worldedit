"""Write test schematics: Sponge v2 and v3, Litematica, legacy MCEdit."""
import gzip
import struct
import sys
import pathlib

TAG_END, TAG_BYTE, TAG_SHORT, TAG_INT, TAG_LONG = 0, 1, 2, 3, 4
TAG_BYTE_ARRAY, TAG_STRING, TAG_LIST, TAG_COMPOUND, TAG_INT_ARRAY, TAG_LONG_ARRAY = (
    7,
    8,
    9,
    10,
    11,
    12,
)


def s16(value):
    return struct.pack(">h", value)


def i32(value):
    return struct.pack(">i", value)


def i64(value):
    return struct.pack(">q", value)


def string(text):
    raw = text.encode("utf-8")
    return struct.pack(">H", len(raw)) + raw


def tag(kind, payload):
    return struct.pack(">B", kind) + payload


def compound(entries):
    """`entries` are `(name, tag(kind, payload))`: the tag re-named."""
    body = b""
    for name, serialized in entries:
        body += serialized[:1] + string(name) + serialized[1:]
    return body + tag(TAG_END, b"")


def named(root_name, body):
    return tag(TAG_COMPOUND, string(root_name) + body)


def byte(value):
    return TAG_BYTE, struct.pack(">b", value)


def short(value):
    return TAG_SHORT, s16(value)


def integer(value):
    return TAG_INT, i32(value)


def text(value):
    return TAG_STRING, string(value)


def byte_array(values):
    return TAG_BYTE_ARRAY, i32(len(values)) + bytes(values)


def int_array(values):
    return TAG_INT_ARRAY, i32(len(values)) + b"".join(i32(v) for v in values)


def long_array(values):
    return TAG_LONG_ARRAY, i32(len(values)) + b"".join(i64(v) for v in values)


def list_of(kind, payloads):
    return TAG_LIST, struct.pack(">B", kind) + i32(len(payloads)) + b"".join(payloads)


def compound_payload(entries):
    return compound(entries)


def varints(values):
    out = bytearray()
    for value in values:
        while True:
            byte_value = value & 0x7F
            value >>= 7
            if value:
                out.append(byte_value | 0x80)
            else:
                out.append(byte_value)
                break
    return out


def sponge(version, size, palette, indices):
    width, height, length = size
    data = varints(indices)
    palette_entries = [(name, tag(*integer(index))) for name, index in palette]
    palette_tag = tag(TAG_COMPOUND, compound(palette_entries))
    if version == 3:
        blocks = (
            "Blocks",
            tag(
                TAG_COMPOUND,
                compound(
                    [
                        ("Palette", palette_tag),
                        ("Data", tag(*byte_array(data))),
                    ]
                ),
            ),
        )
        body = compound(
            [
                ("Version", tag(*integer(3))),
                ("DataVersion", tag(*integer(5023))),
                ("Width", tag(*short(width))),
                ("Height", tag(*short(height))),
                ("Length", tag(*short(length))),
                ("Offset", tag(*int_array([0, 0, 0]))),
                blocks,
            ]
        )
    else:
        body = compound(
            [
                ("Version", tag(*integer(2))),
                ("DataVersion", tag(*integer(5023))),
                ("Width", tag(*short(width))),
                ("Height", tag(*short(height))),
                ("Length", tag(*short(length))),
                ("Offset", tag(*int_array([0, 0, 0]))),
                ("PaletteMax", tag(*integer(len(palette)))),
                ("Palette", palette_tag),
                ("BlockData", tag(*byte_array(data))),
            ]
        )
    return gzip.compress(named("Schematic", body), mtime=0)


def packed(values, bits):
    total = len(values) * bits
    longs = [0] * ((total + 63) // 64)
    for index, value in enumerate(values):
        start = index * bits
        slot, shift = divmod(start, 64)
        longs[slot] |= (value & ((1 << bits) - 1)) << shift
        if shift + bits > 64:
            longs[slot + 1] |= value >> (64 - shift)
    return [v - (1 << 64) if v >= 1 << 63 else v for v in longs]


def litematic():
    palette = [
        [("Name", tag(*text("minecraft:stone")))],
        [("Name", tag(*text("minecraft:air")))],
        [
            ("Name", tag(*text("minecraft:oak_stairs"))),
            (
                "Properties",
                tag(
                    TAG_COMPOUND,
                    compound(
                        [
                            ("facing", tag(*text("east"))),
                            ("half", tag(*text("bottom"))),
                            ("shape", tag(*text("straight"))),
                            ("waterlogged", tag(*text("false"))),
                        ]
                    ),
                ),
            ),
        ],
    ]
    # 3x2x3: a stone floor at y=0, one stairs block above.
    values = []
    for y in range(2):
        for z in range(3):
            for x in range(3):
                if y == 0:
                    values.append(0)
                elif (x, z) == (1, 1):
                    values.append(2)
                else:
                    values.append(1)
    main = compound(
        [
            (
                "Position",
                tag(
                    TAG_COMPOUND,
                    compound(
                        [("x", tag(*integer(0))), ("y", tag(*integer(0))), ("z", tag(*integer(0)))]
                    ),
                ),
            ),
            (
                "Size",
                tag(
                    TAG_COMPOUND,
                    compound(
                        [("x", tag(*integer(3))), ("y", tag(*integer(2))), ("z", tag(*integer(3)))]
                    ),
                ),
            ),
            (
                "BlockStatePalette",
                tag(*list_of(TAG_COMPOUND, [compound(entry) for entry in palette])),
            ),
            ("BlockStates", tag(*long_array(packed(values, 2)))),
            ("TileEntities", tag(*list_of(TAG_COMPOUND, []))),
        ]
    )
    # A mirrored region: Size.x = -2 puts its blocks at x = 5, 6 around Position.x = 6.
    mirror = compound(
        [
            (
                "Position",
                tag(
                    TAG_COMPOUND,
                    compound(
                        [("x", tag(*integer(6))), ("y", tag(*integer(0))), ("z", tag(*integer(0)))]
                    ),
                ),
            ),
            (
                "Size",
                tag(
                    TAG_COMPOUND,
                    compound(
                        [("x", tag(*integer(-2))), ("y", tag(*integer(2))), ("z", tag(*integer(2)))]
                    ),
                ),
            ),
            (
                "BlockStatePalette",
                tag(*list_of(TAG_COMPOUND, [compound(entry) for entry in palette[:1]])),
            ),
            ("BlockStates", tag(*long_array([]))),
            ("TileEntities", tag(*list_of(TAG_COMPOUND, []))),
        ]
    )
    metadata = compound(
        [
            ("Name", tag(*text("harbor"))),
            (
                "EnclosingSize",
                tag(
                    TAG_COMPOUND,
                    compound(
                        [("x", tag(*integer(8))), ("y", tag(*integer(2))), ("z", tag(*integer(3)))]
                    ),
                ),
            ),
        ]
    )
    body = compound(
        [
            ("Version", tag(*integer(6))),
            ("MinecraftDataVersion", tag(*integer(5023))),
            ("Metadata", tag(TAG_COMPOUND, metadata)),
            (
                "Regions",
                tag(
                    TAG_COMPOUND,
                    compound([("main", tag(TAG_COMPOUND, main)), ("mirror", tag(TAG_COMPOUND, mirror))]),
                ),
            ),
        ]
    )
    return gzip.compress(named("", body), mtime=0)


def mcedit():
    width, height, length = 2, 2, 2
    ids = [1, 1, 1, 1, 5, 5, 5, 5]  # stone, then oak planks
    data = [0, 0, 0, 0]
    body = compound(
        [
            ("Width", tag(*short(width))),
            ("Height", tag(*short(height))),
            ("Length", tag(*short(length))),
            ("Blocks", tag(*byte_array(ids))),
            ("Data", tag(*byte_array(data))),
            ("WEOffset", tag(*int_array([0, 0, 0]))),
        ]
    )
    return gzip.compress(named("Schematic", body), mtime=0)


def main():
    out = pathlib.Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    size = (3, 2, 3)
    palette = [
        ("minecraft:stone", 0),
        ("minecraft:dirt", 1),
        ("minecraft:oak_stairs[facing=east,half=bottom,shape=straight,waterlogged=false]", 2),
        ("minecraft:glass", 3),
    ]
    indices = []
    for y in range(2):
        for z in range(3):
            for x in range(3):
                if y == 0:
                    indices.append(0 if x in (0, 2) or z in (0, 2) else 1)
                elif (x, z) == (1, 1):
                    indices.append(2)
                else:
                    indices.append(3)
    (out / "harbor-v2.schem").write_bytes(sponge(2, size, palette, indices))
    (out / "harbor-v3.schem").write_bytes(sponge(3, size, palette, indices))
    (out / "harbor.litematic").write_bytes(litematic())
    (out / "harbor.schematic").write_bytes(mcedit())
    for path in sorted(out.iterdir()):
        print(f"{path.name}: {path.stat().st_size} bytes")


if __name__ == "__main__":
    main()
