import helpers.Digests.Cipher;
import helpers.Digests.HttpsURLConnection;
import helpers.Digests.MessageDigest;
import helpers.Gateway;
import helpers.Runtime;

public class Decoys {
    void repositorySymbols(String command, String name) {
        Runtime.getRuntime().exec(command);
        Gateway gateway = new Gateway();
        gateway.executeQuery("SELECT * FROM users WHERE name = '" + name + "'");
        gateway.eval(command);
        gateway.readObject();
        MessageDigest.getInstance("MD5");
        Cipher.getInstance("DES");
        HttpsURLConnection.setDefaultHostnameVerifier((host, session) -> true);
    }
}
