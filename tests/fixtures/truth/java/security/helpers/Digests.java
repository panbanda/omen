package helpers;

public class Digests {
    public static class MessageDigest {
        public static MessageDigest getInstance(String name) {
            return new MessageDigest();
        }
    }

    public static class Cipher {
        public static Cipher getInstance(String name) {
            return new Cipher();
        }
    }

    public static class HttpsURLConnection {
        public static void setDefaultHostnameVerifier(Object verifier) {
        }
    }
}
