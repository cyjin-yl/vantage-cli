#!/bin/sh
# Run as root on CT102 only, after the integration owner drains Vantage jobs:
#   sh .gitea/runner/deploy.sh
# No registration, API credential, image build, pull or external fetch occurs.
set -eu
[ "$(id -u)" -eq 0 ] || { echo 'Run on CT102 as root' >&2; exit 1; }
base=/tank/docker/doescontainer/binds/vantage-runner
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cached=/root/.cache/act/fbbfa0afa15394b4519a8aa6e6e7db098439546f39cb09b3b6c125c214fd93da
checkout_cached=/root/.cache/act/a73a5a8327b9339595b6329aef98816745aa9873ff08864dad7d2385890d49a4

# Keep existing registration and the preseeded 2.4GB image. Fail rather than
# downloading replacement inputs or silently registering another runner.
if [ ! -s "$base/identity/.runner" ]; then
  # One-time migration of our existing registration, not a new runner.
  test -s "$base/.runner"
  install -d -m 0700 "$base/identity"
  cp -p "$base/.runner" "$base/identity/.runner"
  chmod 0600 "$base/identity/.runner"
fi
docker image inspect vantage-ci-job:latest >/dev/null
docker image inspect gitea/act_runner:0.2.13 >/dev/null

# Preserve only the already-cached checkout v4/uploader v3 runtimes, not
# their Git/config metadata. Node checkout uses REST when git is absent.
# These hashes bind both runtimes to the exact inspected local bytes.
# Uploader source revision: ff15f0306b3f739f7b6fd43fb5d26cd321bd4de5.
# Stage before replacing the old runner: its writable cache is ephemeral.
stage=$(mktemp -d "$base/.ci-actions.XXXXXX")
trap 'rm -rf "$stage"' EXIT HUP INT TERM
mkdir -p "$stage/upload-artifact/dist"
if [ -d "$base/ci-actions/upload-artifact" ]; then
  cp "$base/ci-actions/upload-artifact/action.yml" "$stage/upload-artifact/action.yml"
  cp "$base/ci-actions/upload-artifact/dist/index.js" "$stage/upload-artifact/dist/index.js"
  cp "$base/ci-actions/upload-artifact/LICENSE" "$stage/upload-artifact/LICENSE"
else
  docker cp "vantage-runner:$cached/action.yml" "$stage/upload-artifact/action.yml"
  docker cp "vantage-runner:$cached/dist/index.js" "$stage/upload-artifact/dist/index.js"
  docker cp "vantage-runner:$cached/LICENSE" "$stage/upload-artifact/LICENSE"
fi
mkdir -p "$stage/checkout/dist"
if [ -d "$base/ci-actions/checkout" ]; then
  cp "$base/ci-actions/checkout/action.yml" "$stage/checkout/action.yml"
  cp "$base/ci-actions/checkout/dist/index.js" "$stage/checkout/dist/index.js"
  cp "$base/ci-actions/checkout/LICENSE" "$stage/checkout/LICENSE"
else
  docker cp "vantage-runner:$checkout_cached/action.yml" "$stage/checkout/action.yml"
  docker cp "vantage-runner:$checkout_cached/dist/index.js" "$stage/checkout/dist/index.js"
  docker cp "vantage-runner:$checkout_cached/LICENSE" "$stage/checkout/LICENSE"
fi
cat > "$stage/SHA256SUMS" <<'SUMS'
6188f6991491ed38977347cdaad0b0cd921a6d6232892363c91f433c22954f4f  checkout/action.yml
ff743df9891fd49ffdf6e6bb11ad574bce26a13862ede59474201ef4c9ae029e  checkout/dist/index.js
1516e0d1018602f83eba56c8394f60729e8163f5fde2f38efbb1019e8bdddfb3  upload-artifact/action.yml
234e2ac32fa481faddfcc6892c9731eb496754676b5b419368373ac691197986  upload-artifact/dist/index.js
SUMS
(cd "$stage" && sha256sum --check SHA256SUMS)
chmod -R a+rX "$stage"

# A timestamped rollback directory contains the old service/config only.
# Credential-bearing runner.env and the parent directory are not copied.
backup="$base/deploy-backup-$(date -u +%Y%m%dT%H%M%SZ)"
install -d -m 0700 "$backup"
cp /etc/systemd/system/vantage-act-runner.service "$backup/"
cp "$base/config.yaml" "$backup/"
install -d "$base/cargo-target" "$base/ci-actions"
cp -R "$stage/." "$base/ci-actions/"
install -m 0644 "$here/config.yaml" "$base/config.yaml"
install -m 0644 "$here/vantage-act-runner.service" /etc/systemd/system/vantage-act-runner.service
systemctl daemon-reload
systemctl restart vantage-act-runner.service

# Deliberately leave the old TLS bridge running until the integration owner
# sees a successful replacement run, mounts and artifact readback. Then:
# systemctl disable --now vantage-action-mirror.service
# Remove ONLY the owned bridge unit/script, runner-hosts and tls/ directory
# after checking no other consumer uses that listener/certificate. Remove
# legacy runner.env only after checking its credential is not shared. Do not
# revoke the owner's broader PAT or change Gitea global authentication.
printf '%s\n' "Installed; rollback config/service: $backup" \
  'Integration must verify: exact entrypoint, socket absence, cache bind,' \
  'offline cargo steps, CPU/memory bounds, green run and three-binary artifact.'
