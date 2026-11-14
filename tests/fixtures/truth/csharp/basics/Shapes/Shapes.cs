using System;

namespace Shop.Shapes;

public interface IShape
{
    double Area();
}

public class Circle : IShape
{
    private readonly double radius;

    public Circle(double radius)
    {
        this.radius = radius;
    }

    public double Area()
    {
        return this.Scaled(Math.PI);
    }

    private double Scaled(double factor)
    {
        return factor * radius * radius;
    }

    protected virtual string Name()
    {
        return "circle";
    }

    [Obsolete]
    internal double Diameter()
    {
        return 2 * radius;
    }
}

internal class Ring : Circle
{
    public Ring(double radius) : base(radius)
    {
    }

    protected override string Name()
    {
        return "ring of " + base.Name();
    }
}

public class Square : IShape
{
    private readonly double side;

    public Square(double side)
    {
        this.side = side;
    }

    public double Area()
    {
        return side * side;
    }

    public double Perimeter()
    {
        return 4 * Edge();
    }

    internal double Edge()
    {
        return side;
    }
}

public enum Unit
{
    Metre,
    Foot
}

public struct Point
{
    public double X;
    public double Y;

    public Point(double x, double y)
    {
        X = x;
        Y = y;
    }

    public double Norm()
    {
        return Math.Sqrt(X * X + Y * Y);
    }
}

public record Size(double Width, double Height)
{
    public double Total()
    {
        return Width * Height;
    }
}

internal class Pair
{
    private Circle lead;
    private Circle[] rest = new Circle[0];
    private Pair next;
    internal Circle Spare { get; set; }

    public Pair(Circle lead)
    {
        this.lead = lead;
    }

    internal double LeadArea()
    {
        return this.lead.Area();
    }

    internal double NextArea()
    {
        return next.lead.Area();
    }

    internal double SpareArea()
    {
        return Spare.Area();
    }

    internal double RestArea()
    {
        double total = 0;
        foreach (var member in rest)
        {
            total += member.Area();
        }
        return total;
    }
}
