#!/usr/bin/env bash
# Generate a fresh ed25519 keypair under tests/fixtures/.
#
# The keypair is gitignored (see .gitignore: tests/fixtures/id_ed25519*).
# CI and local developers should regenerate before running the integration
# tests so we never reuse a checked-in private key. The integration tests
# will call this script automatically if the keypair is missing.
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
KEY="$DIR/id_ed25519"

# Always regenerate to keep the fixture key short-lived.
rm -f "$KEY" "$KEY.pub"
ssh-keygen -t ed25519 -N "" -f "$KEY" -C safessh-test-key >/dev/null
chmod 600 "$KEY"
echo "Generated $KEY"
