source lib.sh
source ./lib.sh
shopt -s expand_aliases
alias greet_alias=greet

greet() {
    echo hi
}

countdown() {
    if (( $1 > 0 )); then
        countdown $(( $1 - 1 ))
    fi
}

if [[ -n "${VERBOSE:-}" ]]; then
    report() {
        echo verbose
    }
fi

helper() {
    echo helper
}
export -f helper

run() {
    greet_alias
    countdown 3
    report
    command helper
    ( lib_func )
}
