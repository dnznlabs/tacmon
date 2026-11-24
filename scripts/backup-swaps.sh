#!/usr/bin/env bash
#
# Snapshots a fast bridge provider's swap records, both directions. A swap's record is its safety state: lose the disk and
# the swaps in flight are lost with it, so the records belong on another disk or another machine.
#
#   bash scripts/backup-swaps.sh <destination-dir>
#   DATA_DIR=/var/lib/tacit-fastbridge-lp KEEP=96 bash scripts/backup-swaps.sh /mnt/backup/tacit
#   make backup DEST=/mnt/backup/tacit
#
# Each run writes <destination-dir>/swaps-<UTC time>.tar.gz, readable by this user only, and removes
# its own snapshots beyond the newest KEEP. Run it from cron or a systemd timer, as often as a swap
# changes phase: every few minutes while the provider is busy.
#
# The records are the provider's. Keep the snapshots as private as the data directory itself.
#
set -euo pipefail

DEST="${1:-}"
DATA_DIR="${DATA_DIR:-/var/lib/tacit-fastbridge-lp}"
KEEP="${KEEP:-48}"

die() { echo "backup-swaps: $*" >&2; exit 1; }

[ -n "$DEST" ] || die "usage: backup-swaps.sh <destination-dir>"
[ -d "$DATA_DIR/swaps" ] || die "no swap records at $DATA_DIR/swaps (set DATA_DIR)"
[ -r "$DATA_DIR/swaps" ] || die "$DATA_DIR/swaps cannot be read by $(id -un)"
case "$KEEP" in ''|*[!0-9]*|0) die "KEEP must be a whole number above zero" ;; esac

umask 077
mkdir -p "$DEST"
# The same disk is not a backup of it.
if [ "$(stat -c %d "$DEST")" = "$(stat -c %d "$DATA_DIR/swaps")" ]; then
  echo "backup-swaps: warning: $DEST is on the same filesystem as the records" >&2
fi

OUT="$DEST/swaps-$(date -u +%Y%m%dT%H%M%SZ).tar.gz"
# Written beside its final name and moved into place, so a snapshot that exists is a whole one.
# The reverse side keeps its swaps apart; a provider with that side off has no such directory.
PARTS=(swaps)
[ -d "$DATA_DIR/reverse/swaps" ] && PARTS+=(reverse/swaps)
tar -czf "$OUT.partial" -C "$DATA_DIR" "${PARTS[@]}"
# Read back before it counts: an archive that cannot be listed protects nothing.
tar -tzf "$OUT.partial" > /dev/null || die "the snapshot could not be read back"
mv "$OUT.partial" "$OUT"

# Only this script's own snapshots, by name, oldest first; the newest KEEP stay.
find "$DEST" -maxdepth 1 -type f -name 'swaps-*.tar.gz' | sort | head -n "-$KEEP" | while IFS= read -r old; do
  rm -f -- "$old"
done

echo "$OUT ($(tar -tzf "$OUT" | grep -c '\.json$') records, $(du -h "$OUT" | cut -f1))"
