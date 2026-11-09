"""Sets the DMG window's saved frame in a .DS_Store, in place: python3 -I dmg-window.py <.DS_Store> <x> <y> <w> <h>

Finder keeps the window's settings in the root .DS_Store as a binary plist ("bwsp"). A tiling
window manager on the Mac that builds the DMG snaps the window Finder opens, and Finder saves the
snapped frame, so installer.sh writes the one it meant here. The edit keeps every length the same
(the frame string is written without spaces, then padded with them after its first comma;
NSRectFromString skips them), so nothing else in the
file moves. It also turns the tab bar off. x, y: the window's bottom-left in Cocoa points.
"""
import sys


def main(path, x, y, w, h):
    data = bytearray(open(path, "rb").read())
    at = data.find(b"bwsp")
    start = data.find(b"bplist00", at)
    if at < 0 or start < 0:
        sys.exit("dmg-window: no window settings in " + path)
    length = int.from_bytes(data[start - 4:start], "big")
    plist = memoryview(data)[start:start + length]

    # The trailer: offset and reference sizes, object count, top object, offset table.
    off_size, ref_size = plist[-26], plist[-25]
    count = int.from_bytes(plist[-24:-16], "big")
    top = int.from_bytes(plist[-16:-8], "big")
    table = int.from_bytes(plist[-8:], "big")
    offset = [int.from_bytes(plist[table + i * off_size:table + (i + 1) * off_size], "big") for i in range(count)]

    def body(o):
        """(start, length) of a string or a dict's entries, after its marker and length."""
        marker = plist[o]
        n, p = marker & 0x0F, o + 1
        if n == 0x0F:
            size = 1 << (plist[p] & 0x0F)
            n, p = int.from_bytes(plist[p + 1:p + 1 + size], "big"), p + 1 + size
        return p, n

    def string(ref):
        p, n = body(offset[ref])
        return bytes(plist[p:p + n]).decode("ascii") if plist[offset[ref]] >> 4 == 0x5 else None

    p, n = body(offset[top])
    refs = [int.from_bytes(plist[p + i * ref_size:p + (i + 1) * ref_size], "big") for i in range(2 * n)]
    values = {string(k): v for k, v in zip(refs[:n], refs[n:])}

    frame = values.get("WindowBounds")
    if frame is None or plist[offset[frame]] >> 4 != 0x5:
        sys.exit("dmg-window: no WindowBounds string")
    p, n = body(offset[frame])
    want = f"{{{{{x},{y}}},{{{w},{h}}}}}"
    if len(want) > n:
        sys.exit(f"dmg-window: {want!r} doesn't fit in {n} characters")
    padded = want.replace(",", "," + " " * (n - len(want)), 1)
    plist[p:p + n] = padded.encode("ascii")

    # Booleans are one byte: 0x08 false, 0x09 true.
    tabs = values.get("ShowTabView")
    if tabs is not None and plist[offset[tabs]] in (0x08, 0x09):
        plist[offset[tabs]] = 0x08

    open(path, "wb").write(data)
    print(f"dmg-window: {padded!r}")


if __name__ == "__main__":
    main(sys.argv[1], *(int(a) for a in sys.argv[2:6]))
