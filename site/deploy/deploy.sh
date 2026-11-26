#!/usr/bin/env bash
# Builds the website image, pushes it to ghcr.io and runs it on Nomad.
#
#   site/deploy/deploy.sh            build, push, run
#   site/deploy/deploy.sh --build    build only, to try it locally
#   site/deploy/deploy.sh --plan     build, push, and show what Nomad would change
#
# The Nomad address, its certificates and the registry token come from secretd
# (scopes heaper/nomad and heaper/ghcr); nothing is read from the shell or a
# .env file. Docker itself must be logged in to ghcr.io to push.
set -euo pipefail

GHCR_USERNAME=janlunge

# Second stage, inside `sec run` with an empty environment: hand secretd's values to the Nomad CLI.
if [[ "${POG_DEPLOY_STAGE:-}" == "nomad" ]]; then
  export NOMAD_ADDR="${ADDR:?heaper/nomad/ADDR missing in secretd}"
  export NOMAD_TLS_SERVER_NAME="${TLS_SERVER_NAME:?heaper/nomad/TLS_SERVER_NAME missing in secretd}"
  export NOMAD_VAR_ghcr_username="$GHCR_USERNAME"
  export NOMAD_VAR_ghcr_token="${GHCR_TOKEN:?heaper/ghcr/GHCR_TOKEN missing in secretd}"
  nomad_bin="$POG_NOMAD_BIN"
  unset ADDR TLS_SERVER_NAME GHCR_TOKEN CACERT CLIENT_CERT CLIENT_KEY POG_DEPLOY_STAGE POG_NOMAD_BIN
  exec "$nomad_bin" "$@"
fi

here="$(cd "$(dirname "$0")" && pwd)"
site="$(dirname "$here")"
version="$(git -C "$site" rev-parse --short HEAD)"
[[ -z "$(git -C "$site" status --porcelain -- "$site")" ]] || version="$version-dirty"
image="ghcr.io/janlunge/pogv2-site:$version"

if [[ "${1:-}" == "--build" ]]; then
  docker build -t "$image" "$site"
  echo "Built $image. Try it: docker run --rm -p 8080:80 $image"
  exit 0
fi

nomad_bin="$(command -v nomad)" || { echo "nomad CLI not found" >&2; exit 127; }
sec_bin="$(command -v sec)" || { echo "sec CLI not found" >&2; exit 127; }

# The cluster runs on x86, whatever this machine is.
docker buildx build --platform linux/amd64 -t "$image" --push "$site"

action=(job run)
[[ "${1:-}" == "--plan" ]] && action=(job plan)

exec env -i HOME="$HOME" PATH=/usr/bin:/bin TMPDIR="${TMPDIR:-/tmp}" \
  POG_DEPLOY_STAGE=nomad POG_NOMAD_BIN="$nomad_bin" \
  "$sec_bin" run \
  --scope heaper/nomad --scope heaper/ghcr \
  --file NOMAD_CACERT=sec://heaper/nomad/CACERT \
  --file NOMAD_CLIENT_CERT=sec://heaper/nomad/CLIENT_CERT \
  --file NOMAD_CLIENT_KEY=sec://heaper/nomad/CLIENT_KEY \
  -- /bin/bash "$0" "${action[@]}" -var="VERSION=$version" "$here/pogv2-site.hcl"
