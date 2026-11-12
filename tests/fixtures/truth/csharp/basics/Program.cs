using System;
using System.Linq;
using Shop.Shapes;
using Tools = Shop.Util.Helpers;
using static Shop.Other.Others;

namespace Shop;

public class Program
{
    private readonly int offset;

    public Program(int offset)
    {
        this.offset = offset;
    }

    internal int Offset()
    {
        return this.offset;
    }

    static int Local()
    {
        return 4;
    }

    static int Run()
    {
        int Inner()
        {
            return 5;
        }
        int total = Local() + Tools.Helper() + OnlyHere() + Inner();
        List<int> numbers = new[] { 1, 2 }.Select(n => TwiceLocal(n)).ToList();
        Func<int, int> add = n => n + 1;
        return total + add(numbers.Count);
    }

    static int TwiceLocal(int value)
    {
        return value * 2;
    }

    static double Measure(Square size)
    {
        return size.Area();
    }

    static double Infer()
    {
        var circle = new Circle(1.0);
        return circle.Area();
    }

    static int Shadow()
    {
        Func<int> Helper = () => 9;
        return Helper();
    }

    static int Imported()
    {
        return Helper();
    }

    static double Dynamic(IShape shape)
    {
        return shape.Area();
    }

    static int External()
    {
        var seen = new List<int>();
        seen.Add(1);
        return Math.Max(seen.Count, 0);
    }

    static int Qualified()
    {
        return Shop.Util.Helpers.Helper();
    }

    static int Doubled()
    {
        return Tools.Twice(2);
    }

    static double DoubledReal()
    {
        return Tools.Twice(2.0);
    }

    private sealed class Counter
    {
        private int count;

        public int Next()
        {
            count += 1;
            return count;
        }
    }

    static int Tally()
    {
        var counter = new Counter();
        return counter.Next();
    }

    static string Describe()
    {
        return "shape";
    }

    public static void Main()
    {
        double size = Measure(new Square(2.0));
        var program = new Program(1);
        Console.WriteLine(Run() + " " + Shadow() + " " + Describe() + " " + size + " " + program.Offset());
        Console.WriteLine(Dynamic(new Circle(1.0)) + Infer() + External() + Imported() + Qualified());
        Console.WriteLine(Doubled() + DoubledReal() + Tally());
    }
}
