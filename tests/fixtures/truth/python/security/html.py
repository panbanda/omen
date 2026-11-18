from django.utils.html import escape
from django.utils.safestring import mark_safe
from markupsafe import Markup as Trusted


def unsafe_html_output(text):
    mark_safe(text)
    Trusted(text)
    escape(text)
    value = "mark_safe(text)"
    # mark_safe(text)
    mark_safe("<br>")
    return value
