using System.IO;
using System.Runtime.Serialization.Formatters.Binary;

public class Extra
{
    public void Constructed(Stream stream)
    {
        new BinaryFormatter().Deserialize(stream);
    }
}
