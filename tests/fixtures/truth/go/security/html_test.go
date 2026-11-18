package main

import (
	"html/template"
	"testing"
)

func TestHTML(t *testing.T) {
	_ = template.HTML(t.Name())
}
