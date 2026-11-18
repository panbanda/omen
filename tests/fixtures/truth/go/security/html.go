package main

import (
	"html/template"

	trusted "html/template"
)

func unsafeHTMLOutput(text string) {
	_ = template.HTML(text)
	_ = trusted.JS(text)
	_ = template.HTMLEscapeString(text)
	_ = "template.HTML(text)"
	// template.HTML(text)
	_ = template.HTML("<br>")
}
