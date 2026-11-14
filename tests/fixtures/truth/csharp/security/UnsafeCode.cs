public class UnsafeCode
{
    public unsafe int Read(int* pointer)
    {
        unsafe
        {
            return *pointer;
        }
    }

    public int Plain(int value)
    {
        checked
        {
            return value + 1;
        }
    }

    public string Describe()
    {
        var text = "unsafe { }";
        // unsafe { }
        return text;
    }
}
