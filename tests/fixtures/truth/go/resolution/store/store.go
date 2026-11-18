package store

func Open() string { return "store" }

func Must[T any](value T) T { return value }
