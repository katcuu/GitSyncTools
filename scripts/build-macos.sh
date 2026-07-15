#!/usr/bin/env bash
set -Eeuo pipefail

version="${1:-}"
create_updater_artifacts="${2:-false}"

usage() {
  echo "Usage: $0 <version tag, for example v0.3.4> [true|false]" >&2
}

if [[ ! "$version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then
  usage
  exit 2
fi
if [[ "$create_updater_artifacts" != "true" && "$create_updater_artifacts" != "false" ]]; then
  usage
  exit 2
fi
if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script can only run on macOS." >&2
  exit 1
fi
if [[ "$(uname -m)" != "arm64" ]]; then
  echo "Only Apple Silicon (arm64) Macs are supported." >&2
  exit 1
fi

for command in git node npm cargo; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "Required command was not found: $command" >&2
    exit 1
  fi
done

if [[ "$create_updater_artifacts" == "true" && -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  echo "Updater artifacts were requested, but TAURI_SIGNING_PRIVATE_KEY is not set." >&2
  exit 1
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$script_dir/.." && pwd)"
cd "$repository_root"
if [[ ! -d .git ]]; then
  echo "Not a Git repository: $repository_root" >&2
  exit 1
fi

echo "Fetching tag: $version"
git fetch --force --tags origin
git rev-parse --verify --quiet "refs/tags/$version^{commit}" >/dev/null

echo "WARNING: all uncommitted changes, untracked files, and ignored build files in this repository will be permanently deleted." >&2
git reset --hard HEAD
git clean -fdx
git checkout --detach --force "$version"
git reset --hard "$version"

expected_version="${version#v}"
package_version="$(node -p "require('./package.json').version")"
if [[ "$package_version" != "$expected_version" ]]; then
  echo "Version mismatch: tag is $version but package.json is $package_version" >&2
  exit 1
fi

echo "Installing locked dependencies for $version"
npm ci
npm test

temporary_directory=""
temporary_config=""
cleanup() {
  if [[ -n "$temporary_directory" && -d "$temporary_directory" ]]; then
    rm -rf "$temporary_directory"
  fi
}
trap cleanup EXIT

build_arguments=(run tauri -- build --target aarch64-apple-darwin)
if [[ "$create_updater_artifacts" == "true" ]]; then
  build_arguments+=(--bundles app,dmg)
else
  temporary_directory="$(mktemp -d "${TMPDIR:-/tmp}/gitsynctools-build.XXXXXX")"
  temporary_config="$temporary_directory/config.json"
  printf '%s' '{"bundle":{"createUpdaterArtifacts":false}}' > "$temporary_config"
  build_arguments+=(--bundles dmg --config "$temporary_config")
fi

echo "Building macOS $version (updater artifacts: $create_updater_artifacts)"
npm "${build_arguments[@]}"

echo "Build completed:"
echo "  $repository_root/src-tauri/target/aarch64-apple-darwin/release/bundle/dmg"
if [[ "$create_updater_artifacts" == "true" ]]; then
  echo "  $repository_root/src-tauri/target/aarch64-apple-darwin/release/bundle/macos"
fi
