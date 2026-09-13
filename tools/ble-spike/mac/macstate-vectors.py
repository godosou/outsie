#!/usr/bin/env python3
"""Known-answer vectors for the Mac's state beacon (repose-macstate-v2).

This is the independent third implementation. presence-verify.swift and
MacStateVectorTest.kt both check themselves against the tags this prints;
neither of them is allowed to be the source of its own expected values,
because a vector produced by the code it checks agrees with itself no
matter what either of them says.

Pre-image and tag (docs/design/phone-key.html, chapter 14):

    msg = "repose-macstate-v2 beacon" ‖ keyId(1) ‖ macId(2 BE) ‖ counter(8 BE) ‖ state(1)
    tag = HMAC-SHA256(K, msg)[0..8)

state  0 open · 1 locked · 2 measuring near · 3 near done, walk away
       4 measuring far · 5 done · 6 both ends too alike · 7 no phone heard

Standard library only (hmac, hashlib) so there is nothing to install and
nothing shared with the two implementations under test.

Usage:  python3 macstate-vectors.py
"""

import hashlib
import hmac
import sys

LABEL = b"repose-macstate-v2 beacon"
TAG_LEN = 8

KEY = bytes(range(0x20))  # 000102...1e1f
KEY_ID = 1
COUNTER = 58_000_000
MAC_ID = 0xABCD


def tag(key: bytes, key_id: int, mac_id: int, counter: int, state: int) -> str:
    if not 0 <= state <= 0xFF:
        raise ValueError(f"state must fit one byte, got {state}")
    msg = (
        LABEL
        + key_id.to_bytes(1, "big")
        + mac_id.to_bytes(2, "big")
        + counter.to_bytes(8, "big", signed=True)
        + state.to_bytes(1, "big")
    )
    return hmac.new(key, msg, hashlib.sha256).digest()[:TAG_LEN].hex()


# The vectors that were already written down on both sides before state grew
# past 0/1. If any of these move, the pre-image above is wrong -- stop, do not
# "fix" the expected values.
EXPECTED = {
    (MAC_ID, 1): "d16d729bc487b663",
    (MAC_ID, 0): "ee8a24179231a934",
    (0x0001, 1): "eedf1ad32139ab89",
}


def main() -> int:
    failures = 0
    for (mac_id, state), want in EXPECTED.items():
        got = tag(KEY, KEY_ID, mac_id, COUNTER, state)
        ok = got == want
        failures += 0 if ok else 1
        print(f"  {'ok  ' if ok else 'FAIL'} macId={mac_id:04x} state={state} -> {got}"
              + ("" if ok else f" (expected {want})"))
    if failures:
        print(f"{failures} known vector(s) did not reproduce; the pre-image is wrong.")
        return 1

    print()
    print(f"key      = {KEY.hex()}")
    print(f"keyId    = {KEY_ID}")
    print(f"macId    = {MAC_ID:#06x}")
    print(f"counter  = {COUNTER}")
    print()
    for state in range(8):
        print(f"  state={state} -> {tag(KEY, KEY_ID, MAC_ID, COUNTER, state)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
