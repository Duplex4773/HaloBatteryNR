"""Convert our native icon's 64px BGRA bitmap to an embedded notification PNG.

Standard library only; this tool never reads machine or device information.
"""
from pathlib import Path
import struct
import zlib

root = Path(__file__).resolve().parents[1]
source = (root / "crates/app/app.ico").read_bytes()
reserved, kind, count = struct.unpack_from("<HHH", source)
assert (reserved, kind, count) == (0, 1, 1)
width, height = source[6], source[7]
offset = struct.unpack_from("<I", source, 18)[0]
header_size, dib_width, dib_height, planes, depth, compression = struct.unpack_from("<IiiHHI", source, offset)
assert (header_size, dib_width, dib_height, planes, depth, compression) == (40, width, height * 2, 1, 32, 0)
pixels = memoryview(source)[offset + 40:offset + 40 + width * height * 4]
rows = bytearray()
for y in reversed(range(height)):
    rows.append(0)
    for x in range(width):
        b, g, r, a = pixels[(y * width + x) * 4:(y * width + x + 1) * 4]
        rows.extend((r, g, b, a))

def chunk(name, data):
    return struct.pack(">I", len(data)) + name + data + struct.pack(">I", zlib.crc32(name + data))

png = b"\x89PNG\r\n\x1a\n"
png += chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(rows, 9))
png += chunk(b"IEND", b"")
(root / "crates/app/application.png").write_bytes(png)
print(f"Wrote {width}x{height} notification icon ({len(png)} bytes)")
