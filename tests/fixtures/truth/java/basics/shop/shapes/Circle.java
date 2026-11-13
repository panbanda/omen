package shop.shapes;

public class Circle implements Shape {
    private final double radius;

    public Circle(double radius) {
        this.radius = radius;
    }

    @Override
    public double area() {
        return this.scaled(Math.PI);
    }

    private double scaled(double factor) {
        return factor * radius * radius;
    }

    protected String name() {
        return "circle";
    }

    double diameter() {
        return 2 * radius;
    }
}

class Ring extends Circle {
    Ring(double radius) {
        super(radius);
    }

    @Override
    protected String name() {
        return "ring of " + super.name();
    }
}
