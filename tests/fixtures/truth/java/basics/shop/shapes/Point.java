package shop.shapes;

public record Point(double x, double y) {
    public double norm() {
        return Math.sqrt(x * x + y * y);
    }
}
