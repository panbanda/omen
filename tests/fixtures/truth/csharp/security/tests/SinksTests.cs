using System.Data.SqlClient;
using System.Diagnostics;
using System.Net.Http;
using System.Runtime.Serialization.Formatters.Binary;
using System.Security.Cryptography;
using Microsoft.CodeAnalysis.CSharp.Scripting;

public class SinksTests
{
    public void TestEverything(BinaryFormatter formatter, System.IO.Stream stream, string name)
    {
        Process.Start("sh");
        CSharpScript.EvaluateAsync(name);
        formatter.Deserialize(stream);
        new SqlCommand("SELECT * FROM users WHERE name = '" + name + "'");
        MD5.Create();
        DES.Create();
        var handler = HttpClientHandler.DangerousAcceptAnyServerCertificateValidator;
        var token = "AKIAIOSFODNN7EXAMPLE";
    }
}
