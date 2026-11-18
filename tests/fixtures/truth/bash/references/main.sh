#!/bin/bash

audit() {
    eval "$AUDIT_COMMAND"
}

start() {
    trap audit EXIT
    trap 'audit' INT
}

shadowed() {
    local audit="$1"
    printf '%s\n' "$audit"
}

other_file() {
    trap cleanup TERM
}

text_only() {
    # trap audit HUP
    echo "trap audit HUP"
    echo audit | xargs -n1 echo
}

start
shadowed "$1"
other_file
text_only
