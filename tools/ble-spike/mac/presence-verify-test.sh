#!/bin/bash
#
# End-to-end tests for the Mac half of authenticated presence, with no radio and
# no phone: OpenSSL mints the beacons, presence-verify judges them.
#
# What this can prove: that a tag minted with the paired key verifies, that one
# minted with any other key does not, that the window bounds are what the design
# says, and that a key file anyone could have tampered with is refused.
#
# What it CANNOT prove: that the phone builds the same pre-image byte for byte.
# Both halves could agree with OpenSSL here and still disagree with each other if
# they disagreed about, say, the counter's endianness -- and the symptom would be
# a phone that simply never unlocks anything, which looks like bad radio range.
# Only tests/e2e/impersonation_test.sh, with the real phone, closes that gap.
#
#   sudo -v && tools/ble-spike/mac/presence-verify-test.sh

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VERIFY="${HERE}/presence-verify"
LABEL="repose-presence-v1 beacon"
WINDOW=30

pass=0; fail=0; skip=0
ok()  { printf '  ok   %s\n' "$1"; pass=$((pass+1)); }
bad() { printf '  FAIL %s -- %s\n' "$1" "${2:-}"; fail=$((fail+1)); }

echo "presence-verify"

[ -x "${VERIFY}" ] || swiftc -O -o "${VERIFY}" "${VERIFY}.swift" || {
    echo "  could not build presence-verify" >&2; exit 2; }

# The key must be root:wheel 0600 for the verifier to touch it, so building one
# needs root. Without a cached credential the file-backed cases are SKIPPED and
# counted as skipped -- never as passed. A test run that quietly drops its
# hardest cases and still says "all green" is how this project talked itself
# into a wrong answer before.
HAVE_SUDO=0
sudo -n true 2>/dev/null && HAVE_SUDO=1
if [ "${HAVE_SUDO}" = 0 ]; then
    echo "  (no cached sudo credential: the key-file cases will be skipped."
    echo "   run 'sudo -v' first to include them)"
fi
maybe_skip() { skip=$((skip+1)); printf '  SKIP %s -- needs sudo\n' "$1"; }

SANDBOX="$(mktemp -d "${TMPDIR:-/tmp}/pverify.XXXXXX")"
cleanup() { [ "${HAVE_SUDO}" = 1 ] && sudo rm -rf "${SANDBOX}"; rm -rf "${SANDBOX}"; }
trap cleanup EXIT

KEY_HEX="$(openssl rand -hex 32)"
OTHER_HEX="$(openssl rand -hex 32)"
if [ "${HAVE_SUDO}" = 1 ]; then
    sudo mkdir -p "${SANDBOX}/keys"
    sudo chown root:wheel "${SANDBOX}/keys"
    sudo install -m 600 -o root -g wheel /dev/null "${SANDBOX}/keys/presence-key.1"
    printf '%s\n' "${KEY_HEX}" | sudo tee "${SANDBOX}/keys/presence-key.1" >/dev/null
else
    mkdir -p "${SANDBOX}/keys"
fi

# The pre-image, assembled as bytes: label ‖ keyId(1) ‖ counter(8, big-endian).
tag_for() {
    local key_hex="$1" key_id="$2" counter="$3"
    {
        printf '%s' "${LABEL}" | xxd -p -c 256 | tr -d '\n'
        printf '%02x%016x' "${key_id}" "${counter}"
    } | xxd -r -p \
      | openssl dgst -sha256 -mac HMAC -macopt "hexkey:${key_hex}" -binary \
      | xxd -p -c 256 | cut -c1-16
}

NOW=1740000000
C0=$(( NOW / WINDOW ))

# $1 = csv row, $2 = key dir -> prints the verdict field
# Written without an array: /bin/bash on macOS is 3.2, where expanding an empty
# array under `set -u` is an error, not an empty list. The first version of this
# passed under Homebrew bash and failed under the shebang's -- exactly the kind
# of difference that makes a suite green in one place and red in another.
verdict() {
    if [ "${HAVE_SUDO}" = 1 ]; then
        printf '%s\n' "$1" | sudo "${VERIFY}" --key-dir "${2:-${SANDBOX}/keys}" \
            --fixed-now "${NOW}" 2>/dev/null | tail -1 | awk -F, '{print $NF}'
    else
        printf '%s\n' "$1" | "${VERIFY}" --key-dir "${2:-${SANDBOX}/keys}" \
            --fixed-now "${NOW}" 2>/dev/null | tail -1 | awk -F, '{print $NF}'
    fi
}

row() { printf '1000,-55,aabbccdd,1,%s,%s' "$1" "$2"; }

if [ "${HAVE_SUDO}" = 1 ]; then
    # 1. The paired key, current window.
    v="$(verdict "$(row 1 "$(tag_for "${KEY_HEX}" 1 "${C0}")")")"
    [ "$v" = VALID ] && ok "a beacon minted with the paired key verifies" \
                     || bad "paired key verifies" "got ${v}"

    # 2. ±1 window, which is the skew and re-mint tolerance.
    for d in -1 1; do
        v="$(verdict "$(row 1 "$(tag_for "${KEY_HEX}" 1 "$(( C0 + d ))")")")"
        [ "$v" = VALID ] && ok "window ${d} is accepted (clock skew tolerance)" \
                         || bad "window ${d}" "got ${v}"
    done

    # 3. Beyond ±1 it must not. This bound IS the replay defence: nothing else
    #    limits how long a captured beacon stays useful.
    for d in -2 2 -120; do
        v="$(verdict "$(row 1 "$(tag_for "${KEY_HEX}" 1 "$(( C0 + d ))")")")"
        [ "$v" = INVALID ] && ok "window ${d} is refused (replay bound)" \
                           || bad "window ${d} refused" "got ${v}"
    done

    # 4. Any other key fails. This is the imposter, in arithmetic form.
    v="$(verdict "$(row 1 "$(tag_for "${OTHER_HEX}" 1 "${C0}")")")"
    [ "$v" = INVALID ] && ok "a beacon minted with a different key is refused" \
                       || bad "wrong key refused" "got ${v}"

    # 5. A tag one nibble off.
    good="$(tag_for "${KEY_HEX}" 1 "${C0}")"
    flipped="$(printf '%s' "${good}" | sed 's/^./0/')"
    [ "${flipped}" = "${good}" ] && flipped="$(printf '%s' "${good}" | sed 's/^./1/')"
    v="$(verdict "$(row 1 "${flipped}")")"
    [ "$v" = INVALID ] && ok "a one-nibble-different tag is refused" \
                       || bad "flipped tag refused" "got ${v}"

    # 6. A keyId this Mac has no key for is NOKEY, not a pass.
    v="$(verdict "$(row 9 "$(tag_for "${KEY_HEX}" 9 "${C0}")")")"
    [ "$v" = NOKEY ] && ok "an unknown keyId is NOKEY" || bad "unknown keyId" "got ${v}"

    # 7. The keyId is bound into the tag, so a tag minted for slot 9 must not
    #    verify when replayed as slot 1 -- otherwise one key covers every slot.
    sudo install -m 600 -o root -g wheel /dev/null "${SANDBOX}/keys/presence-key.9"
    printf '%s\n' "${KEY_HEX}" | sudo tee "${SANDBOX}/keys/presence-key.9" >/dev/null
    v="$(verdict "$(row 1 "$(tag_for "${KEY_HEX}" 9 "${C0}")")")"
    [ "$v" = INVALID ] && ok "keyId is bound into the tag (slot 9's tag fails as slot 1)" \
                       || bad "keyId binding" "got ${v}"
    sudo rm -f "${SANDBOX}/keys/presence-key.9"

    # 8. A key file whose owner is not root, or that is group/world readable, is
    #    refused rather than used. A key anyone can rewrite is a key anyone can
    #    become the paired phone with, and silently repairing the mode would
    #    hide that something had already been able to write it.
    sudo mkdir -p "${SANDBOX}/loose"
    sudo install -m 644 -o root -g wheel /dev/null "${SANDBOX}/loose/presence-key.1"
    printf '%s\n' "${KEY_HEX}" | sudo tee "${SANDBOX}/loose/presence-key.1" >/dev/null
    v="$(verdict "$(row 1 "$(tag_for "${KEY_HEX}" 1 "${C0}")")" "${SANDBOX}/loose")"
    [ "$v" = NOKEY ] && ok "a 0644 key file is refused, not used" || bad "0644 refused" "got ${v}"

    sudo mkdir -p "${SANDBOX}/mine"
    sudo install -m 600 -o "$(id -u)" -g "$(id -g)" /dev/null "${SANDBOX}/mine/presence-key.1"
    printf '%s\n' "${KEY_HEX}" > "${SANDBOX}/mine/presence-key.1"
    v="$(verdict "$(row 1 "$(tag_for "${KEY_HEX}" 1 "${C0}")")" "${SANDBOX}/mine")"
    [ "$v" = NOKEY ] && ok "a non-root-owned key file is refused" || bad "owner check" "got ${v}"
else
    for c in "the paired key verifies" "±1 window tolerance" "windows beyond ±1 are refused" \
             "a different key is refused" "a one-nibble-different tag is refused" \
             "an unknown keyId is NOKEY" "keyId is bound into the tag" \
             "a 0644 key file is refused" "a non-root-owned key file is refused"; do
        maybe_skip "${c}"
    done
    echo "  (the arithmetic half of these runs key-free in: presence-verify --self-test)"
fi

# 9. Malformed input is marked, not dropped. A pipeline that silently loses rows
#    looks exactly like a phone that went quiet. Needs no key.
v="$(verdict "1000,-55,aabbccdd,-,-,-")"
[ "$v" = MALFORMED ] && ok "a payload-less advertisement is MALFORMED" \
                     || bad "no payload" "got ${v}"
v="$(verdict "something that is not our csv")"
[ "$v" = MALFORMED ] && ok "an unparseable line is passed through marked" \
                     || bad "unparseable line" "got ${v}"

# 10. Nothing in the verifier may treat an unreadable key as permission to pass.
grep -nE 'return "VALID"' "${VERIFY}.swift" | grep -vq 'constantTimeEqual' \
    && bad "VALID is only returned after a constant-time compare" "found another path" \
    || ok "VALID is only returned after a constant-time compare"

echo
if [ "${skip}" -gt 0 ]; then
    echo "${pass} passed, ${fail} failed, ${skip} SKIPPED (run 'sudo -v' to include them)"
else
    echo "${pass} passed, ${fail} failed"
fi
[ "${fail}" = 0 ]
