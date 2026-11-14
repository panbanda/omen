package main

import "unsafe"

func TestUnsafe(value *int) {
	_ = unsafe.Pointer(value)
}
