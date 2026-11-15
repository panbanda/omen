using System;
using System.Collections.Generic;

class Program
{
    static void Helper() {}

    static void Output()
    {
        Console.WriteLine("x");
        Convert.ToInt32("3");
    }

    static void Collections()
    {
        List<int> numbers = new List<int>();
        numbers.Add(1);
    }

    static int Decoy()
    {
        Helper();
        return Math.Max(1, 2);
    }
}
