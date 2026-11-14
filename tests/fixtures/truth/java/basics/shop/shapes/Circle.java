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

class Pair {
    private Circle lead;
    private Circle[] rest = new Circle[0];
    private Pair next;

    Pair(Circle lead) {
        this.lead = lead;
    }

    double leadArea() {
        return this.lead.area();
    }

    double nextArea() {
        return next.lead.area();
    }

    double restArea() {
        double total = 0;
        for (var member : rest) {
            total += member.area();
        }
        return total;
    }
}
