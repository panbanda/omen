package store

import "testing"

func TestOpen(t *testing.T) {
	if Open() == "" {
		t.Fail()
	}
}
