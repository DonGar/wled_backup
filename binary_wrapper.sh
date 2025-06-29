#!/bin/sh

BACKUP_DELAY="${BACKUP_DELAY:-1d}"   # Default to 1 day
SEARCH_SECS="${SEARCH_SECS:-10}"     # Default to 10 seconds

while true; do
    /bin/wled_backup --search-secs "${SEARCH_SECS}" --out-dir /backup
    echo "sleeping for ${BACKUP_DELAY}"
    sleep "${BACKUP_DELAY}"
done
