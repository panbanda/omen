package sample

type Server struct {
	Host string
	Port int
}

func NewServer(host string, port int) *Server {
	return &Server{Host: host, Port: port}
}

func (s *Server) Address() string {
	return fmt.Sprintf("%s:%d", s.Host, s.Port)
}

func (s *Server) validate() error {
	if s.Host == "" {
		return fmt.Errorf("host required")
	}
	if s.Port <= 0 || s.Port > 65535 {
		return fmt.Errorf("invalid port")
	}
	return nil
}

func maxOf(a, b int) int {
	if a > b {
		return a
	}
	return b
}
