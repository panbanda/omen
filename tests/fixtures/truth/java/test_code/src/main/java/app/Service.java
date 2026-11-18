package app;

public class Service {
    public int production() {
        return 1;
    }

    public int testConnection() {
        return production();
    }

    @Override
    public String toString() {
        return "service";
    }
}
