package app;

import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.ValueSource;

class Checks {
    private Service service;

    @BeforeEach
    void prepare() {
        service = new Service();
    }

    @Test
    void givesOne() {
        check(1);
    }

    @ParameterizedTest
    @ValueSource(ints = {1, 2})
    void acceptsEachValue(int value) {
        check(value);
    }

    private void check(int value) {
        if (service.production() > value) {
            throw new AssertionError();
        }
    }
}
