package shop.shapes;

public class Square implements Shape {
    private final double side;

    public Square(double side) {
        this.side = side;
    }

    @Override
    public double area() {
        return side * side;
    }

    public double perimeter() {
        return 4 * edge();
    }

    double edge() {
        return side;
    }
}
