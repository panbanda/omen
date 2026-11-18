import java.io.IOException;

public class Other {
    public static void cleanup() {
        try {
            Runtime.getRuntime().exec("rm " + System.getenv("TARGET"));
        } catch (IOException error) {
            throw new IllegalStateException(error);
        }
    }
}
