using System;
using System.Diagnostics;
using System.Threading.Tasks;
using Microsoft.AspNetCore.Mvc;

[ApiController]
[Route("api/jobs")]
public class JobController : ControllerBase
{
    private Library library;

    private void Audit()
    {
        Process.Start(Environment.GetEnvironmentVariable("EVENT"));
    }

    [HttpPost]
    public string CreateJob(string value)
    {
        void LoadModule()
        {
            Process.Start(value);
        }
        Task.Run(LoadModule);
        Task.Run(Audit);
        return "ok";
    }

    [HttpPost("shadow")]
    public string Shadowed(Action Audit)
    {
        Task.Run(Audit);
        return "ok";
    }

    [HttpPost("other")]
    public string OtherFile()
    {
        Task.Run(library.Cleanup);
        return "ok";
    }

    [HttpPost("text")]
    public string TextOnly()
    {
        // Task.Run(Audit);
        return "Audit";
    }
}
