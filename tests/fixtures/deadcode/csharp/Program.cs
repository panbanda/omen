using System;

public class Program
{
    private static void LiveDirect() { }
    private static void LiveValue() { }
    private static void DeadPlain() { }
    private static void DeadCaller()
    {
        DeadCallee();
    }
    private static void DeadCallee() { }
    private static void DeadUnresolved() { }
    public static void LivePublic() { }

    private void LiveMethod() { }
    private void DeadMethod() { }

    static void Main(string[] args)
    {
        LiveDirect();
        Action handler = LiveValue;
        handler();
        Program tool = new Program();
        tool.LiveMethod();
        Other.Run(null);
    }
}
