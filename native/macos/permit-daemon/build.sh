#!/bin/bash
#
# Build repose-permitd (root daemon) and repose-permit-probe (test client) with
# clang alone -- no Rust, no python. Run on a Mac with the Command Line Tools.
#
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p build

CFLAGS="-O2 -Wall -Wextra -Werror -std=c11"

echo "==> repose-permitd (daemon: CoreFoundation + Security + libbsm)"
# The daemon needs the Security framework for SecCode / SecRequirement, and
# libbsm for the audit_token_to_* accessors. CoreFoundation for CFData/CFString.
clang $CFLAGS \
    -o build/repose-permitd \
    repose-permitd.c repose_peer_verify.c \
    -framework CoreFoundation -framework Security -lbsm

echo "==> repose-permit-probe (client: plain sockets, no frameworks)"
clang $CFLAGS \
    -o build/repose-permit-probe \
    repose-permit-probe.c repose_permit_client.c

echo "Built:"
echo "  build/repose-permitd"
echo "  build/repose-permit-probe"
