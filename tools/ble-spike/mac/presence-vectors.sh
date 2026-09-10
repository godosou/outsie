#!/bin/bash
# Generate the known-answer vectors embedded in presence-verify.swift, using
# OpenSSL rather than any of this project's own code.
#
# The point is cross-implementation agreement. A self-test that checks Swift
# against numbers Swift produced would pass just as happily with the wrong
# pre-image on both sides -- and a wrong pre-image is silent: the phone's tags
# would never verify, which looks exactly like a phone that is out of range.
#
#   tools/ble-spike/mac/presence-vectors.sh
#
# Prints Swift literals. Paste them into `let vectors` in presence-verify.swift.

set -euo pipefail

LABEL="repose-presence-v1 beacon"
TAG_LEN=8

# key_hex key_id counter
CASES=(
  "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f 1 0"
  "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f 1 58000000"
  "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff 7 1"
)

# The pre-image is bytes, not text, so it is assembled in hex and fed to openssl
# with -binary on both ends: label ‖ keyId(1) ‖ counter(8, big-endian).
preimage_hex() {
  local key_id="$1" counter="$2"
  printf '%s' "${LABEL}" | xxd -p -c 256
  printf '%02x' "${key_id}"
  printf '%016x' "${counter}"
}

for c in "${CASES[@]}"; do
  read -r key_hex key_id counter <<< "${c}"
  msg="$(preimage_hex "${key_id}" "${counter}" | tr -d '\n')"
  tag="$(printf '%s' "${msg}" | xxd -r -p \
        | openssl dgst -sha256 -mac HMAC -macopt "hexkey:${key_hex}" -binary \
        | xxd -p -c 256 | cut -c1-$((TAG_LEN * 2)))"
  printf '    Vector(keyHex: "%s",\n           keyId: %s, counter: %s, tagHex: "%s"),\n' \
    "${key_hex}" "${key_id}" "${counter}" "${tag}"
done
