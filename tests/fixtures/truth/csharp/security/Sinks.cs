using System.Data.SqlClient;
using System.Diagnostics;
using System.Net.Http;
using System.Runtime.Serialization.Formatters.Binary;
using System.Security.Cryptography;
using Microsoft.CodeAnalysis.CSharp.Scripting;
using Command = System.Data.SqlClient.SqlCommand;
using Handler = System.Net.Http.HttpClientHandler;
using Legacy = System.Runtime.Serialization.Formatters.Binary.BinaryFormatter;
using Script = Microsoft.CodeAnalysis.CSharp.Scripting.CSharpScript;
using Shell = System.Diagnostics.Process;
using Sha = System.Security.Cryptography.SHA1;
using Stream = System.Security.Cryptography.RC2;

public class Sinks
{
    public void CommandExecution(string command)
    {
        Process.Start(command);
        Shell.Start(command);
        Process.GetCurrentProcess();
        var text = "Process.Start(command)";
        // Process.Start(command)
    }

    public void CodeEvaluation(string expression)
    {
        CSharpScript.EvaluateAsync(expression);
        Script.RunAsync(expression);
        CSharpScript.Create("1");
        var text = "CSharpScript.EvaluateAsync(expression)";
        // CSharpScript.EvaluateAsync(expression)
    }

    public void UnsafeDeserialization(BinaryFormatter formatter, Legacy legacy, System.IO.Stream stream)
    {
        formatter.Deserialize(stream);
        legacy.Deserialize(stream);
        formatter.Serialize(stream, "value");
        var text = "formatter.Deserialize(stream)";
        // formatter.Deserialize(stream)
    }

    public void SqlStringConstruction(string name)
    {
        new SqlCommand("SELECT * FROM users WHERE name = '" + name + "'");
        new Command("SELECT * FROM users WHERE name = '" + name + "'");
        new SqlCommand("SELECT * FROM users WHERE name = @name");
        var text = "new SqlCommand(query)";
        // new SqlCommand(query)
    }

    public void WeakHash()
    {
        MD5.Create();
        Sha.Create();
        SHA256.Create();
        var text = "MD5.Create()";
        // MD5.Create()
    }

    public void WeakCipher()
    {
        DES.Create();
        Stream.Create();
        Aes.Create();
        var text = "DES.Create()";
        // DES.Create()
    }

    public void TlsVerificationDisabled()
    {
        var first = HttpClientHandler.DangerousAcceptAnyServerCertificateValidator;
        var second = Handler.DangerousAcceptAnyServerCertificateValidator;
        var third = HttpClientHandler.DefaultProxy;
        var text = "HttpClientHandler.DangerousAcceptAnyServerCertificateValidator";
        // HttpClientHandler.DangerousAcceptAnyServerCertificateValidator
    }

    public void HardcodedCredential()
    {
        var token = "AKIAIOSFODNN7EXAMPLE";
        var name = "AKIAEXAMPLE";
        // AKIAIOSFODNN7EXAMPLE
    }
}
