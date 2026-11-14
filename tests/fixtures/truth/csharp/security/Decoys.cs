using Helpers;

public class Decoys
{
    public void RepositorySymbols(string command, string name, BinaryFormatter formatter, System.IO.Stream stream, HttpClientHandler handler)
    {
        Process.Start(command);
        CSharpScript.EvaluateAsync(command);
        formatter.Deserialize(stream);
        new SqlCommand("SELECT * FROM users WHERE name = '" + name + "'");
        MD5.Create();
        DES.Create();
        handler.ServerCertificateCustomValidationCallback = HttpClientHandler.DangerousAcceptAnyServerCertificateValidator;
    }
}
