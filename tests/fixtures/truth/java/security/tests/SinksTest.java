import java.io.ObjectInputStream;
import java.security.MessageDigest;
import java.sql.Statement;
import javax.crypto.Cipher;
import javax.net.ssl.HttpsURLConnection;
import javax.script.ScriptEngine;

public class SinksTest {
    void testEverything(ScriptEngine engine, ObjectInputStream input, Statement statement, String name) throws Exception {
        Runtime.getRuntime().exec("sh");
        engine.eval(name);
        input.readObject();
        statement.executeQuery("SELECT * FROM users WHERE name = '" + name + "'");
        MessageDigest.getInstance("MD5");
        Cipher.getInstance("DES");
        HttpsURLConnection.setDefaultHostnameVerifier((host, session) -> true);
        String token = "AKIAIOSFODNN7EXAMPLE";
    }
}
