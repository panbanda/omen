public class Shapes {
    static class Circle {
        double radius;

        double area() {
            return 3.14 * radius * radius;
        }
    }

    static class Square {
        double side;

        double area() {
            return side * side;
        }
    }
}
