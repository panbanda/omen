import java.io.InputStream;
import java.io.ObjectInputStream;

public class Extra {
    void constructed(InputStream stream) throws Exception {
        new ObjectInputStream(stream).readObject();
    }
}
