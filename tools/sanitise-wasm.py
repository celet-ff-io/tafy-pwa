#!/usr/bin/env python3
"""sanitise-wasm.py — strip build-machine absolute paths from shipped wasm.

WHY: dx/manganis embeds the asset manifest with the ABSOLUTE build
path of each asset source (e.g. /home/<user>/.../crates/pwa/assets/
main.css) inside the wasm's data segments (rustc panic locations are
already handled by --remap-path-prefix; this string is produced by the
bundler, not rustc). A bundle-only distribution (A16) must not leak
the machine's filesystem layout.

SAFETY: the manifest is consumed by dx's bundler at BUILD time; the
running app resolves assets by their bundled name (main-<hash>.css)
which is untouched. The patch is byte-for-byte EQUAL LENGTH inside a
data segment, so no wasm section lengths or offsets change.

USAGE: sanitise-wasm.py FILE [FILE...]
Every occurrence of a printable path starting with "/home/" is
replaced by an equal-length "/build/" + filler path. Re-runnable
(idempotent — after the first run there is nothing left to match).
"""

import re
import sys

# printable path run: /home/<user>/... up to the first byte that
# cannot appear in a filesystem path literal embedded in a data
# segment (control chars, NUL, quotes, high bytes)
PATTERN = re.compile(rb"/home/[\x20-\x2b\x2d-\x7e]+")


def patch(data: bytes) -> tuple[bytes, int]:
    count = 0

    def repl(m: re.Match) -> bytes:
        nonlocal count
        count += 1
        orig = m.group(0)
        # equal length, neutral, obviously synthetic
        filler = b"/build/" + b"x" * (len(orig) - len(b"/build/"))
        # keep the file's basename readable for post-mortem clarity
        tail = orig.rsplit(b"/", 1)[-1]
        return filler[: len(filler) - len(tail)] + tail

    return PATTERN.sub(repl, data), count


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    status = 0
    for path in sys.argv[1:]:
        try:
            with open(path, "rb") as f:
                data = f.read()
        except OSError as e:
            print(f"{path}: unreadable ({e})")
            status = 1
            continue
        patched, count = patch(data)
        if count:
            try:
                with open(path, "wb") as f:
                    f.write(patched)
            except OSError as e:
                print(f"{path}: unwritable ({e})")
                status = 1
                continue
        print(f"{path}: {count} path(s) patched")
    return status


if __name__ == "__main__":
    raise SystemExit(main())
