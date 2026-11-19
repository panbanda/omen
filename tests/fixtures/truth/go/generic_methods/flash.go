package flash

type Flash struct{ message string }

func (f *Flash) Error[T string | []byte](message T) {
	f.message = string(message)
}

func (f *Flash) Reset() {
	f.message = ""
}

func use(f *Flash) {
	f.Error("failed")
	f.Reset()
}
