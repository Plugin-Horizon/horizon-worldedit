"""Count the blocks of a Sponge v2 .schem, to check what a paste wrote."""
import collections
import gzip
import sys

import nbtlib


def varints(data):
    out = []
    value = 0
    shift = 0
    for byte in data:
        value |= (byte & 0x7F) << shift
        if byte & 0x80:
            shift += 7
        else:
            out.append(value)
            value = 0
            shift = 0
    return out


def report(path):
    root = nbtlib.load(path)
    palette = {index: str(name) for name, index in root["Palette"].items()}
    counts = collections.Counter(palette[i] for i in varints(root["BlockData"]))
    size = (int(root["Width"]), int(root["Height"]), int(root["Length"]))
    total = sum(counts.values())
    assert total == size[0] * size[1] * size[2], (total, size)
    print(f"{path.rsplit(chr(92), 1)[-1].rsplit('/', 1)[-1]} {size[0]}x{size[1]}x{size[2]}:")
    for name, count in counts.most_common():
        print(f"    {count:3d}  {name}")


for argument in sys.argv[1:]:
    report(argument)
