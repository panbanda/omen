package helpers

type Gateway struct{}

func (gateway *Gateway) Query(statement string) string {
	return statement
}

func Command(program string) string {
	return program
}

func New() string {
	return "new"
}

func NewCipher(key []byte) []byte {
	return key
}
