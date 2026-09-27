#!/bin/sh
# Prints the licence notices for every crate compiled into the given programs, as
# "package:target" pairs, for example: subutf8-app:x86_64-unknown-linux-musl
# Runs wherever cargo and its crate cache are, in the build containers or the Docker build.
set -eu

echo "SubUTF8 is MIT licensed (see LICENSE). It includes the Rust crates below;"
echo "each is listed with its licence, followed by the licence files it ships."
for program in "$@"; do
    cargo tree --locked -p "${program%%:*}" -e normal --target "${program##*:}" \
        --prefix none --format '{p}|{l}'
done | sed -e 's/ (\*)$//' -e 's/ (proc-macro)//' | grep -v '(/' | sort -u |
    while IFS='|' read -r crate licence; do
        name="${crate% v*}"
        crate_version="${crate##* v}"
        printf '\n==== %s %s (%s)\n' "$name" "$crate_version" "$licence"
        for folder in "${CARGO_HOME:-/usr/local/cargo}"/registry/src/*/"$name-$crate_version"; do
            find "$folder" -maxdepth 1 -type f \
                \( -iname 'LICENSE*' -o -iname 'LICENCE*' -o -iname 'COPYING*' -o -iname 'NOTICE*' \) \
                -exec sh -c 'for file; do printf "\n---- %s\n" "${file##*/}"; cat "$file"; done' sh {} +
        done
    done
