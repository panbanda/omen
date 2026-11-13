#!/bin/bash

code_evaluation() {
    eval "$1"
    printf '%s\n' "$1"
    local text="eval $1"
    # eval "$1"
}

weak_hash() {
    md5sum "$1"
    sha256sum "$1"
    local text="md5sum $1"
    # md5sum "$1"
}

weak_cipher() {
    openssl enc -des -in "$1"
    openssl enc -aes-256-cbc -in "$1"
    local text="openssl enc -des"
    # openssl enc -des -in "$1"
}

tls_verification_disabled() {
    curl -k "$1"
    curl "$1"
    local text="curl -k $1"
    # curl -k "$1"
}

hardcoded_credential() {
    local token="AKIAIOSFODNN7EXAMPLE"
    local name="AKIAEXAMPLE"
    # AKIAIOSFODNN7EXAMPLE
}
