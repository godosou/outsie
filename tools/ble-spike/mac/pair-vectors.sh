#!/bin/bash
#
# Known-answer vectors for repose-pair-v2, computed with OpenSSL.
#
# WHY OPENSSL AND NOT EITHER IMPLEMENTATION
#
# The Swift side and the Kotlin side both have to agree on a transcript, a SAS
# and a key. The tempting shortcut is to run one, write down what it said, and
# make the other match -- which proves they agree and proves nothing about
# whether they agree on the RIGHT thing. Both wrong in the same way passes that
# test. So the answers come from a third implementation neither of them shares
# code with.
#
# Fixed keys, fixed nonces: the whole point is that the numbers do not move.
# These are throwaway test keys and are meant to be in the repository.
#
#   tools/ble-spike/mac/pair-vectors.sh            # print the vectors
#   tools/ble-spike/mac/pair-vectors.sh --json     # machine-readable
#
# The private keys below were generated once with
#   openssl ecparam -name prime256v1 -genkey -noout
# and pasted in. Regenerating them changes every answer, so don't, unless the
# protocol itself changed.

set -euo pipefail

WORK="$(mktemp -d "${TMPDIR:-/tmp}/pairvec.XXXXXX")"
trap 'rm -rf "${WORK}"' EXIT

cat > "${WORK}/mac.pem" <<'PEM'
-----BEGIN EC PRIVATE KEY-----
MHcCAQEEILluBnYYnbXLKEcOTcpIlBOFYD1eYncmmX+5cyF0bmn5oAoGCCqGSM49
AwEHoUQDQgAEzurBpiXDA55eMXY2LisldGHGaos6WyPb5nQkyEs/EgLzTdjDfnJu
BHTSj/mExvrU0xWLqudhoc+lDTV11NKvIQ==
-----END EC PRIVATE KEY-----
PEM

cat > "${WORK}/phone.pem" <<'PEM'
-----BEGIN EC PRIVATE KEY-----
MHcCAQEEIObFlTZCMvILZodtQb4jMTrkCyVWaHV7PAaHg2TDx8CyoAoGCCqGSM49
AwEHoUQDQgAEu6CshmwEDe5jOV3H6pzSrmXfdHXDUpXaByZN42qF3MePMLkwoa8x
R6xjoNkCWT5Tp498mrh3NnBWL1Cv6kvwNQ==
-----END EC PRIVATE KEY-----
PEM

# Fixed nonces, 16 bytes each.
NM="000102030405060708090a0b0c0d0e0f"
NP="f0e0d0c0b0a090807060504030201000"

COMMIT_LABEL="repose-pair-v2 commit"
SAS_LABEL="repose-pair-v2 sas"
KDF_LABEL="repose-pair-v2 presence-key"

hexof() { xxd -p -c 256 | tr -d '\n'; }
asciihex() { printf '%s' "$1" | hexof; }
sha256hex() { xxd -r -p | openssl dgst -sha256 -binary | hexof; }
hmac() { xxd -r -p | openssl dgst -sha256 -mac HMAC -macopt "hexkey:$1" -binary | hexof; }

# SEC1 uncompressed public key, 65 bytes: the last 130 hex chars of the DER
# SubjectPublicKeyInfo are exactly 04 ‖ X ‖ Y.
pubhex() {
  openssl ec -in "$1" -pubout -outform DER 2>/dev/null | hexof | tail -c 130
}

# The 32-byte private scalars, so an implementation can reproduce the whole
# exchange rather than only the hashes. Test keys, deliberately committed.
privhex() {
  openssl ec -in "$1" -text -noout 2>/dev/null \
    | awk '/priv:/{f=1;next} /pub:/{f=0} f' | tr -d ' :\n' | tail -c 64
}
SK_M="$(privhex "${WORK}/mac.pem")"
SK_P="$(privhex "${WORK}/phone.pem")"

PK_M="$(pubhex "${WORK}/mac.pem")"
PK_P="$(pubhex "${WORK}/phone.pem")"

# ECDH shared X. openssl pkeyutl -derive gives exactly the X coordinate for
# P-256, left-padded to 32 bytes -- which is the leading-zero preservation the
# implementations must also get right.
openssl ec -in "${WORK}/phone.pem" -pubout -out "${WORK}/phone.pub" 2>/dev/null
Z="$(openssl pkeyutl -derive -inkey "${WORK}/mac.pem" -peerkey "${WORK}/phone.pub" 2>/dev/null | hexof)"

CP="$(printf '%s%s%s%s' "$(asciihex "${COMMIT_LABEL}")" "${PK_M}" "${PK_P}" "${NP}" | sha256hex)"
SAS_HASH="$(printf '%s%s%s%s%s' "$(asciihex "${SAS_LABEL}")" "${PK_M}" "${PK_P}" "${NM}" "${NP}" | sha256hex)"

# 6 digits from the first four bytes, big-endian, mod 1e6.
FIRST4="$(printf '%s' "${SAS_HASH}" | cut -c1-8)"
DIGITS="$(printf '%06d' $(( 16#${FIRST4} % 1000000 )))"

# HKDF: extract with the transcript hash as salt, expand one block with the
# KDF label as info.
PRK="$(printf '%s' "${Z}" | hmac "${SAS_HASH}")"
K="$(printf '%s01' "$(asciihex "${KDF_LABEL}")" | hmac "${PRK}")"

if [ "${1:-}" = "--json" ]; then
  printf '{\n'
  printf '  "sk_m": "%s",\n' "${SK_M}"
  printf '  "sk_p": "%s",\n' "${SK_P}"
  printf '  "pk_m": "%s",\n' "${PK_M}"
  printf '  "pk_p": "%s",\n' "${PK_P}"
  printf '  "nm": "%s",\n'   "${NM}"
  printf '  "np": "%s",\n'   "${NP}"
  printf '  "commit": "%s",\n' "${CP}"
  printf '  "sas_hash": "%s",\n' "${SAS_HASH}"
  printf '  "digits": "%s",\n' "${DIGITS}"
  printf '  "ecdh_x": "%s",\n' "${Z}"
  printf '  "key": "%s"\n' "${K}"
  printf '}\n'
else
  echo "SK_M     ${SK_M}"
  echo "SK_P     ${SK_P}"
  echo "PK_M     ${PK_M}"
  echo "PK_P     ${PK_P}"
  echo "Nm       ${NM}"
  echo "Np       ${NP}"
  echo "commit   ${CP}"
  echo "sasHash  ${SAS_HASH}"
  echo "digits   ${DIGITS}"
  echo "ecdhX    ${Z}"
  echo "K        ${K}"
fi
