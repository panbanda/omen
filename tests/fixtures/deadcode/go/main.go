package main

type Tool struct{}

func (t Tool) liveMethod() {}
func (t Tool) deadMethod() {}

func liveDirect() {}
func liveValue()  {}
func deadPlain()  {}
func deadCaller() {
	deadCallee()
}
func deadCallee()     {}
func deadUnresolved() {}

func LivePublic() {}

func main() {
	liveDirect()
	handler := liveValue
	handler()
	var tool Tool
	tool.liveMethod()
	run(nil)
}
