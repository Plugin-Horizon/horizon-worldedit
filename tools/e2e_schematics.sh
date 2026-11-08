#!/usr/bin/env bash
# Loads, pastes and saves each schematic format in a real game, headless.
#
# Build a staged game first (see the README), then:
#
#   STAGE=/tmp/horizon-we RESOURCES=~/horizon/.local/horizon-client/26.3 \
#     tools/e2e_schematics.sh
#
# It leaves the saved copies in the scratch world and prints the paths; count
# them with `python3 tools/verify_schem.py <file>...`.
set -uo pipefail

STAGE=${STAGE:-/tmp/horizon-we}
RESOURCES=${RESOURCES:-$(cd "$(dirname "$0")/.." && pwd)/../horizon/.local/horizon-client/26.3}
CTL="$STAGE/target/fast/horizon-ctl.exe"
[ -x "$CTL" ] || CTL="$STAGE/target/fast/horizon-ctl"

scratch=$(mktemp -d)
export HORIZON_HOME="$scratch/game"
export HORIZON_LAUNCHER_HOME="$scratch/launcher"
export XDG_DATA_HOME="$scratch/data"
export XDG_CONFIG_HOME="$scratch/config"
export HORIZON_NO_AUDIO=1
export HORIZON_LAUNCHER_NO_SELF_UPDATE=1
SOCK="$scratch/ctl.sock"
echo "scratch: $scratch"
# The game keeps saves/ under its working directory.
cd "$scratch" || exit 1

ctl() { "$CTL" --socket "$SOCK" "$@"; }

"$CTL" --socket "$SOCK" launch --world WE --creative --offscreen --wait --resources "$RESOURCES" \
  > "$scratch/launch.log" 2>&1 &
for _ in $(seq 1 90); do
  [ -S "$SOCK" ] && break
  sleep 2
done
sleep 6

python3 "$(dirname "$0")/make_fixtures.py" "$scratch/fixtures" > /dev/null
WORLD="$scratch/saves/WE"
mkdir -p "$WORLD/schematics"
cp "$scratch/fixtures"/* "$WORLD/schematics"

ctl tp 0 100 0
ctl wait 60

echo "=== list"
ctl command "//schem list"

echo "=== sponge v2 (0 100 0)"
ctl command "//schem load harbor-v2"
ctl tp 0 100 0
ctl command "//paste"
ctl command "//pos1 0 100 0"
ctl command "//pos2 2 101 2"
ctl command "//schem save verify-v2"

echo "=== sponge v3 (20 100 0)"
ctl command "//schem load harbor-v3"
ctl tp 20 100 0
ctl command "//paste"
ctl command "//pos1 20 100 0"
ctl command "//pos2 22 101 2"
ctl command "//schem save verify-v3"

echo "=== litematica (40 100 0, two regions, one mirrored)"
ctl command "//schem load harbor"
ctl tp 40 100 0
ctl command "//paste"
ctl command "//pos1 40 100 0"
ctl command "//pos2 46 101 2"
ctl command "//schem save verify-litematic"

echo "=== mcedit (60 100 0)"
ctl command "//schem load harbor.schematic"
ctl tp 60 100 0
ctl command "//paste"
ctl command "//pos1 60 100 0"
ctl command "//pos2 61 101 1"
ctl command "//schem save verify-mcedit"

echo "=== a path outside the folder"
ctl command "//schem load $scratch/fixtures/harbor-v3.schem"

ls -la "$WORLD/schematics"
ctl quit > /dev/null 2>&1
echo "saved in $WORLD/schematics"
