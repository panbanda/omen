#!/usr/bin/env bash

source ./lib/util.sh
# shellcheck source=other.sh
. "$(dirname "$0")/other.sh"
source /dev/null

if [[ -n "${FANCY:-}" ]]; then
    source ./lib/fancy.sh
else
    source ./lib/plain.sh
fi

base() {
    echo 4
}

function twice_local {
    echo $(($1 * 2))
}

apply() {
    "$1" "$2"
}

run() {
    base
    twice 2
    only_here
    apply twice_local 3
}

outer() {
    inner() {
        echo 5
    }
    inner
}

substitute() {
    local value
    value=$(helper)
    echo "$value"
}

piped() {
    base | cat
}

greet() {
    echo first
}

shadow() {
    greet
}

greet() {
    echo second
}

dynamic() {
    pick
}

external() {
    echo x | grep -c x
}

describe() {
    echo shape
}

wrong_receiver() {
    git status >/dev/null 2>&1 || true
}

main() {
    run
    outer
    substitute
    piped
    shadow
    dynamic
    external
    describe
    wrong_receiver
}

main "$@"
