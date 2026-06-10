using System;
using System.Collections.Generic;

public class Calculator {
    private int value;
    private string name;

    public int Add(int a, int b) {
        return a + b;
    }

    public int Subtract(int a, int b) {
        return a - b;
    }
}

public static class Utils {
    public static string Format(string template, object arg) {
        return string.Format(template, arg);
    }

    public static int Clamp(int value, int min, int max) {
        return Math.Max(min, Math.Min(max, value));
    }
}
