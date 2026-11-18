package local

type Base struct{}

func (b *Base) JSON() string { return "{}" }

func (b *Base) append(value string) string { return value }

type Missing struct{}

func (m Missing) Error() string { return "missing" }
