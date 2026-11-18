use maud::PreEscaped;
use maud::PreEscaped as Trusted;

pub fn unsafe_html_output(text: String) {
    PreEscaped(text.clone());
    Trusted(text.clone());
    html_escape::encode_text(&text);
    let value = "PreEscaped(text)";
    // PreEscaped(text)
    PreEscaped("<br>");
    let _ = value;
}

#[cfg(test)]
mod tests {
    #[test]
    fn html() {
        maud::PreEscaped(String::new());
    }
}
