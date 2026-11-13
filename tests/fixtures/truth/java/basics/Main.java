import java.util.ArrayList;
import java.util.List;
import java.util.function.*;

import shop.other.*;
import shop.shapes.Circle;
import shop.shapes.Shape;
import shop.shapes.Square;
import shop.util.Util;

import static shop.util.Util.helper;

public class Main {
    private final int base;

    public Main(int base) {
        this.base = base;
    }

    int offset() {
        return this.base;
    }

    static int local() {
        return 4;
    }

    static int run() {
        int total = local() + helper() + Other.onlyHere();
        List<Integer> numbers = List.of(1, 2).stream().map(n -> twiceLocal(n)).toList();
        IntUnaryOperator add = n -> n + 1;
        return total + add.applyAsInt(numbers.size());
    }

    static int twiceLocal(int value) {
        return value * 2;
    }

    static double measure(Square size) {
        return size.area();
    }

    static double infer() {
        var circle = new Circle(1.0);
        return circle.area();
    }

    static int shadow() {
        IntSupplier helper = () -> 9;
        return helper() + helper.getAsInt();
    }

    static double dynamic(Shape shape) {
        return shape.area();
    }

    static int external() {
        List<Integer> seen = new ArrayList<>();
        seen.add(1);
        return Math.max(seen.size(), 0);
    }

    static int qualified() {
        return Util.helper();
    }

    static int wildcard() {
        return Other.helper();
    }

    static int doubled() {
        return Util.twice(2);
    }

    static double doubledReal() {
        return Util.twice(2.0);
    }

    private static class Counter {
        private int count;

        int next() {
            count += 1;
            return count;
        }
    }

    static int tally() {
        Counter counter = new Counter();
        return counter.next();
    }

    static String describe() {
        return "shape";
    }

    public static void main(String[] args) {
        double size = measure(new Square(2.0));
        Main app = new Main(1);
        System.out.println(run() + " " + shadow() + " " + describe() + " " + size + " " + app.offset());
        System.out.println(dynamic(new Circle(1.0)) + infer() + external() + qualified() + wildcard());
        System.out.println(doubled() + doubledReal() + tally());
    }
}
