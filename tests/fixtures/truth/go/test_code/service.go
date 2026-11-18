package service

func Production() int {
	return 1
}

func TestConnection() int {
	return Production()
}

func BenchmarkLike() int {
	return Production()
}
