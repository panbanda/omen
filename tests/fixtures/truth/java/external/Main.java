import java.util.ArrayList;
import java.util.List;

public class Main {
    static void helper() {}

    void strings() {
        System.out.println("x");
        Integer.parseInt("3");
    }

    void collections() {
        List<String> names = new ArrayList<>();
        names.add("a");
    }

    int decoy() {
        helper();
        return Math.max(1, 2);
    }
}
