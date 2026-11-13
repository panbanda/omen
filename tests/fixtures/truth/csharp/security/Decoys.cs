using Helpers;

public class Decoys
{
    public void RepositorySymbols(string command, string name, BinaryFormatter formatter, System.IO.Stream stream)
    {
        Process.Start(command);
        CSharpScript.EvaluateAsync(command);
        formatter.Deserialize(stream);
        new SqlCommand("SELECT * FROM users WHERE name = '" + name + "'");
        MD5.Create();
        DES.Create();
        var handler = HttpClientHandler.DangerousAcceptAnyServerCertificateValidator;
    }
}
