package main

import "crypto/tls"

func mapLiteral() {
	_ = map[string]bool{"InsecureSkipVerify": true}
}

type Settings struct {
	InsecureSkipVerify bool
}

func fromSetting(settings Settings, skip bool) {
	_ = &tls.Config{InsecureSkipVerify: settings.InsecureSkipVerify}
	_ = &Settings{InsecureSkipVerify: skip}
	config := &tls.Config{}
	config.InsecureSkipVerify = skip
	config.InsecureSkipVerify = false
	settings.InsecureSkipVerify = skip
}
