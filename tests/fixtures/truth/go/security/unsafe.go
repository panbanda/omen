package main

import (
	"unsafe"

	raw "unsafe"
)

func memoryUnsafe(value *int) {
	_ = unsafe.Pointer(value)
	_ = raw.Pointer(value)
	_ = len("safe")
	text := "unsafe.Pointer(value)"
	// unsafe.Pointer(value)
	_ = text
}
