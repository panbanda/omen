import javax.net.ssl.HttpsURLConnection;

class Verifiers {
    void compactLambda() {
        HttpsURLConnection.setDefaultHostnameVerifier((host, session)->true);
        HttpsURLConnection.setDefaultHostnameVerifier((host, session) -> trusted(host));
    }

    boolean trusted(String host) {
        return host.isEmpty();
    }
}
