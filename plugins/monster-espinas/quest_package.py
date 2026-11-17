"""Build the Hub 3★ single Espinas hunt in the Forlorn Arena."""

import struct
import zlib
from pathlib import Path

HUB_QUEST = "0010308"  # Hub 3-star single hunt: stats, supplies and rewards
TOWER_QUEST = "0010643"  # Forlorn Arena: map and encounter placements
ESPINAS_QUEST = "0010336"
ESPINAS = 107
QUEST_DATA, QUEST_LINK, MESSAGE = 0x1BBFD18E, 0x00E0BB1C, 0x242BB29A
# QDL: the five boss placements and three small-monster sets follow the header.
ENCOUNTER_LINKS = slice(8, 8 + 8 * 20)
ENCOUNTER_KINDS = (0x2553701D, 0x32CA92F8)
LOOSE_EXTENSIONS = {QUEST_DATA: ".ext", QUEST_LINK: ".qdl", MESSAGE: ".gmd"}
ARC_HEADER = struct.Struct("<4sHHI")
ARC_ENTRY = struct.Struct("<64s4I")
TOWER_SPAWN, ESPINAS_SPAWN = "b_m17em011_00", "b_m17em107_00"
HUB_REWARD, ESPINAS_REWARD = "rem_110020", "rem_110107"
# Item, quantity, percentage in this Hub quest's primary reward roll.
MAIN_REWARDS = ((2981, 1, 8), (2980, 1, 28), (2982, 1, 23), (2983, 1, 8),
                (2985, 1, 1), (413, 1, 14), (460, 1, 8), (432, 5, 10))

TEXT = {
    "chS": ("棘龙", "塔之秘境里睡着一头棘龙。", "狩猎1头棘龙"),
    "chT": ("棘龍", "塔之秘境裡睡著一頭棘龍。", "狩獵1頭棘龍"),
    "eng": ("Espinas", "An Espinas sleeps in the Forlorn Arena.", "Hunt an Espinas"),
    "fre": ("Espinas", "Un Espinas dort dans la Tour interdite.", "Chasser un Espinas"),
    "ger": (
        "Espinas",
        "Ein Espinas schläft im Verbotenen Turm.",
        "Erjage einen Espinas.",
    ),
    "ita": ("Espinas", "Un Espinas dorme nell’Arena desolata.", "Caccia un Espinas"),
    "spa": ("Espinas", "Un Espinas duerme en la Arena Forlorn.", "Caza un Espinas"),
}


def localize(data, tower_message, lang):
    n, size, namesize = struct.unpack_from("<I4xII", data, 24)
    start = 41 + namesize
    if n != 7 or start + size != len(data):
        raise ValueError("unexpected quest GMD layout")
    fields = data[start:].decode().split("\0")
    fields[0], fields[2], fields[4] = TEXT[lang]
    tower_size = struct.unpack_from("<I", tower_message, 32)[0]
    fields[3] = (
        tower_message[-tower_size:].decode().split("\0")[3]
    )  # local small monsters
    body = "\0".join(fields).encode()
    header = bytearray(data[:start].replace(HUB_QUEST.encode(), ESPINAS_QUEST.encode()))
    struct.pack_into("<I", header, 32, len(body))
    return header + body


def check_material_slots(source):
    manifest = Path(__file__).with_name("materials.tsv").read_text().splitlines()
    ids = {int(row.split("\t", 1)[0]) for row in manifest if not row.startswith("#")}
    # These are real saveable slots unused by the retail game. Reject a source
    # mod that already assigned a material to one of them.
    raw = (source / "eng/table/itemData_eng.gmd").read_bytes()
    size = struct.unpack_from("<I", raw, 32)[0]
    labels = raw[-size:].split(b"\0")
    if any(labels[2 * item].lower() != b"dummy" for item in ids):
        raise ValueError("Espinas material slots are occupied")
    if not {i for i, _, _ in MAIN_REWARDS if i >= 2980} <= ids:
        raise ValueError("undeclared quest reward material")


def read_arc(path):
    raw = path.read_bytes()
    if raw[:6] != b"ARC\0\x11\0":
        raise ValueError("expected GU quest ARC v17")
    result = {}
    for i in range(struct.unpack_from("<H", raw, 6)[0]):
        name, kind, size, flags, at = ARC_ENTRY.unpack_from(
            raw, ARC_HEADER.size + ARC_ENTRY.size * i
        )
        key = name.rstrip(b"\0").decode(), kind
        result[key] = flags, zlib.decompress(raw[at : at + size])
    return result


def write_arc(path, entries):
    out = bytearray(ARC_HEADER.pack(b"ARC\0", 17, len(entries), 0))
    out.extend(bytes(ARC_ENTRY.size * len(entries)))
    out.extend(bytes(-len(out) % 0x8000))
    for i, ((name, kind), (flags, data)) in enumerate(entries.items()):
        compressed = zlib.compress(data)
        ARC_ENTRY.pack_into(
            out,
            ARC_HEADER.size + ARC_ENTRY.size * i,
            name.encode(),
            kind,
            len(compressed),
            flags & 0xE0000000 | len(data),
            len(out),
        )
        out.extend(compressed)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(out)


def quest_data(hub, tower):
    data = bytearray(hub.replace(HUB_QUEST.encode(), ESPINAS_QUEST.encode()))
    struct.pack_into("<I", data, 12, int(ESPINAS_QUEST))
    data[20:22] = tower[20:22]  # map and time of day
    struct.pack_into("<H", data, 32, ESPINAS)  # hunt target
    struct.pack_into("<H", data, 100, ESPINAS)  # first spawn
    data[113:165] = tower[113:165]  # four unused encounter slots
    data[204:214] = tower[204:214]  # no random intruder
    data[214:219] = bytes((ESPINAS, 0, 0, 0, 0))  # quest card icons
    return data


def quest_links(hub, tower):
    data = bytearray(hub)
    data[ENCOUNTER_LINKS] = tower[ENCOUNTER_LINKS]
    return (
        data.replace(HUB_QUEST.encode(), ESPINAS_QUEST.encode())
        .replace(HUB_REWARD.encode(), ESPINAS_REWARD.encode())
        .replace(TOWER_SPAWN.encode(), ESPINAS_SPAWN.encode())
    )


def quest_rewards():
    data = bytearray(184)
    struct.pack_into("<fIBB", data, 0, 1.0, 1, 1, len(MAIN_REWARDS))
    for index, row in enumerate((*MAIN_REWARDS, (0, 0, 255))):
        struct.pack_into("<HBB", data, 24 + 4 * index, *row)
    return data


def build_quest(source: Path, dest: Path):
    check_material_slots(source)
    arc_dir = Path("loc/arc/quest")
    arc_path = arc_dir / f"q{ESPINAS_QUEST}.arc"
    if (source / arc_path).exists():
        raise ValueError(f"quest {ESPINAS_QUEST} already exists in the source game")
    hub = read_arc(source / arc_dir / f"q{HUB_QUEST}.arc")
    tower = read_arc(source / arc_dir / f"q{TOWER_QUEST}.arc")

    entries = {
        (name.replace(TOWER_SPAWN, ESPINAS_SPAWN), kind): value
        for (name, kind), value in tower.items()
        if kind in ENCOUNTER_KINDS
    }
    for (name, kind), (flags, data) in hub.items():
        if kind in ENCOUNTER_KINDS:
            continue
        if kind in LOOSE_EXTENSIONS:
            tower_data = tower[name.replace(HUB_QUEST, TOWER_QUEST), kind][1]
            if kind == QUEST_DATA:
                data = quest_data(data, tower_data)
            elif kind == QUEST_LINK:
                data = quest_links(data, tower_data)
            else:
                data = localize(data, tower_data, name.split("\\", 1)[0])
        elif name == rf"quest\rem\{HUB_REWARD}":
            data = quest_rewards()
            name = rf"quest\rem\{ESPINAS_REWARD}"
        entries[name.replace(HUB_QUEST, ESPINAS_QUEST), kind] = flags, data

    # The counter also loads loose EXT, QDL and GMD files. Use the final ARC
    # entries for both outputs so the names and payloads always agree.
    for (name, kind), (_, data) in entries.items():
        if kind in LOOSE_EXTENSIONS:
            path = (dest / Path(*name.split("\\"))).with_suffix(LOOSE_EXTENSIONS[kind])
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    write_arc(dest / arc_path, entries)
