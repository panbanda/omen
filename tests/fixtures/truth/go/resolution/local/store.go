package local

type Store interface {
	Save(path string) error
}

type Disk struct{}

func (d *Disk) Save(path string) error { return nil }
