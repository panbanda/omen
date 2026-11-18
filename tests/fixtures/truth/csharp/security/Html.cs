using System.Net;
using Microsoft.AspNetCore.Html;
using Trusted = Microsoft.AspNetCore.Html.HtmlString;

public class Html
{
    public void UnsafeHtmlOutput(string text)
    {
        new HtmlString(text);
        new Trusted(text);
        WebUtility.HtmlEncode(text);
        var value = "new HtmlString(text)";
        // new HtmlString(text);
        new HtmlString("<br>");
    }
}
