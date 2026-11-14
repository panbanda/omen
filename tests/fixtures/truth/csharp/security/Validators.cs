using System.Net;
using System.Net.Http;

public class Validators
{
    public void Bodies(HttpClientHandler handler)
    {
        handler.ServerCertificateCustomValidationCallback = (m, c, ch, e) => true && e == SslPolicyErrors.None;
        handler.ServerCertificateCustomValidationCallback = (m, c, ch, e) => { return true; };
    }
}
