import java.io.ObjectInputStream;
import java.security.MessageDigest;
import java.sql.Connection;
import java.sql.Statement;
import javax.crypto.Cipher;
import javax.net.ssl.HttpsURLConnection;
import javax.net.ssl.HostnameVerifier;
import javax.script.ScriptEngine;

public class Sinks {
    void commandExecution(String command) throws Exception {
        Runtime.getRuntime().exec(command);
        Runtime.getRuntime().availableProcessors();
        String text = "Runtime.getRuntime().exec(command)";
        // Runtime.getRuntime().exec(command)
    }

    void codeEvaluation(ScriptEngine engine, String expression) throws Exception {
        engine.eval(expression);
        engine.getFactory();
        String text = "engine.eval(expression)";
        // engine.eval(expression)
    }

    void unsafeDeserialization(ObjectInputStream input) throws Exception {
        input.readObject();
        input.readInt();
        String text = "input.readObject()";
        // input.readObject()
    }

    void sqlStringConstruction(Statement statement, Connection connection, String name) throws Exception {
        statement.executeQuery("SELECT * FROM users WHERE name = '" + name + "'");
        connection.prepareStatement("SELECT * FROM users WHERE name = ?");
        String text = "statement.executeQuery(query)";
        // statement.executeQuery(query)
    }

    void weakHash() throws Exception {
        MessageDigest.getInstance("MD5");
        MessageDigest.getInstance("SHA-256");
        String text = "MessageDigest.getInstance(\"MD5\")";
        // MessageDigest.getInstance("MD5")
    }

    void weakCipher() throws Exception {
        Cipher.getInstance("DES/ECB/PKCS5Padding");
        Cipher.getInstance("AES/GCM/NoPadding");
        String text = "Cipher.getInstance(\"DES\")";
        // Cipher.getInstance("DES")
    }

    void tlsVerificationDisabled(HostnameVerifier verifier) {
        HttpsURLConnection.setDefaultHostnameVerifier((host, session) -> true);
        HttpsURLConnection.setDefaultHostnameVerifier(verifier);
        String text = "setDefaultHostnameVerifier((host, session) -> true)";
        // setDefaultHostnameVerifier((host, session) -> true)
    }

    void hardcodedCredential() {
        String token = "AKIAIOSFODNN7EXAMPLE";
        String name = "AKIAEXAMPLE";
        // AKIAIOSFODNN7EXAMPLE
    }
}
