import java.util.List;
import java.util.ArrayList;

public class OrderService {
    private int orderId;
    private String status;

    public void process(List<String> items) {
        for (String item : items) {
            System.out.println(item);
        }
    }

    public int getOrderId() {
        return orderId;
    }
}

public class Helper {
    public static int add(int a, int b) {
        return a + b;
    }

    public static String format(String template, Object... args) {
        return String.format(template, args);
    }
}
