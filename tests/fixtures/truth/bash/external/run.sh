#!/bin/bash
source ./util.sh

helper() {
  echo "x"
}

run() {
  echo "a"
  helper
}

decoy() {
  cd /tmp
}
