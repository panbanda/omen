class Page
  def html_safe
    self
  end
end

module HtmlDecoys
  def self.repository_html
    page = Page.new
    page.html_safe
  end
end
