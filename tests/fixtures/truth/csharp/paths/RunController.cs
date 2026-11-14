using System.Diagnostics;
using Microsoft.AspNetCore.Mvc;

[ApiController]
[Route("api/run")]
public class RunController : ControllerBase
{
    [HttpGet]
    public string Run(string value)
    {
        return Prepare(value);
    }

    private string Prepare(string value)
    {
        return Execute(value);
    }

    private string Execute(string value)
    {
        Process.Start(value);
        return value;
    }

    private void Orphan(string value)
    {
        Process.Start(value);
    }
}
