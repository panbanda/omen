#!/usr/bin/env bash

helper() {
  echo 1
}

test_production() {
  assertEquals 1 "$(helper)"
}

. shunit2
