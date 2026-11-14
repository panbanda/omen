package main

import unsafe "example.com/security/helpers"

func unsafeDecoys(value *int) {
	_ = unsafe.Pointer(value)
}
