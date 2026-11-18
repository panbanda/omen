package main

import template "example.com/security/helpers"

func repositoryHTML(text string) {
	_ = template.HTML(text)
}
