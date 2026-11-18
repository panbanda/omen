using System;
using System.Diagnostics;

public class Other
{
    public static void Cleanup()
    {
        Process.Start(Environment.GetEnvironmentVariable("TARGET"));
    }
}
