module Html
  def self.unsafe_html_output(text)
    text.html_safe
    ERB::Util.html_escape(text)
    value = "text.html_safe"
    # text.html_safe
    "<br>".html_safe
    value
  end
end
