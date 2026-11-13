"""Local game inputs and binary readers shared by the Espinas offline checks."""
from pathlib import Path
import struct
import zlib

REPO = Path(__file__).resolve().parents[3]
VITA_ELF = REPO / "mhfz/eboot.bin.elf"
GU_ELF = REPO / "mhgu/main.elf"


def elf(path):
    data = path.read_bytes()
    ph = struct.unpack_from("<I", data, 28)[0]
    stride, count = struct.unpack_from("<HH", data, 42)
    headers = [struct.unpack_from("<8I", data, ph + i * stride) for i in range(count)]
    return data, [h for h in headers if h[0] == 1]


def assets(path):
    b = path.read_bytes()
    assert b[:6] == b"ARC\0\x11\0"
    result = {}
    for i in range(struct.unpack_from("<H", b, 6)[0]):
        name, k, cs, ds, p = struct.unpack_from("<64s4I", b, 12 + 80 * i)
        key = name.rstrip(b"\0").decode(), k
        v = zlib.decompress(b[p : p + cs])
        assert len(v) == ds & 0x1FFFFFFF and key not in result
        result[key] = v
    return result
