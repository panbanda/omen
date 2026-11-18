using Microsoft.AspNetCore.Html;

public class HtmlTests
{
    public void TestHtml(string text)
    {
        new HtmlString(text);
    }
}
