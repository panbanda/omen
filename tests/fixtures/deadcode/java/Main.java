public class Main {
    private static void liveDirect() {}
    private static void liveValue() {}
    private static void deadPlain() {}
    private static void deadCaller() {
        deadCallee();
    }
    private static void deadCallee() {}
    private static void deadUnresolved() {}
    public static void livePublic() {}

    private void liveMethod() {}
    private void deadMethod() {}

    public static void main(String[] args) {
        liveDirect();
        Runnable handler = Main::liveValue;
        handler.run();
        Main tool = new Main();
        tool.liveMethod();
        Other.run(null);
    }
}
