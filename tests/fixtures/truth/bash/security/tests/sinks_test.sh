#!/bin/bash

test_everything() {
    eval "$1"
    md5sum "$1"
    openssl enc -des -in "$1"
    curl -k "$1"
    local token="AKIAJ3KD92LQ7MZP5XWB"
}
