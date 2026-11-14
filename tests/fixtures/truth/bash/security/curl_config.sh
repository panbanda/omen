#!/bin/bash

fetch() {
    curl -K - http://localhost:8080/status
}
