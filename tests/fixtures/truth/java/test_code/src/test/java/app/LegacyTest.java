package app;

public class LegacyTest extends junit.framework.TestCase {
    public void testProduction() {
        assertEquals(1, build().production());
    }

    private Service build() {
        return new Service();
    }
}
