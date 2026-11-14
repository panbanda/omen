public class UnsafeTests
{
    public unsafe int TestRead(int* pointer)
    {
        unsafe
        {
            return *pointer;
        }
    }
}
