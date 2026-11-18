package service

import "testing"

func helper() int {
	return Production()
}

func Testify() int {
	return helper()
}

func TestProduction(t *testing.T) {
	if helper() != 1 {
		t.Fatal("not one")
	}
}

func BenchmarkProduction(b *testing.B) {
	for i := 0; i < b.N; i++ {
		Production()
	}
}

func FuzzProduction(f *testing.F) {
	f.Fuzz(func(t *testing.T, value int) {
		Production()
	})
}

func ExampleProduction() {
	Production()
}

func TestMain(m *testing.M) {
	m.Run()
}
