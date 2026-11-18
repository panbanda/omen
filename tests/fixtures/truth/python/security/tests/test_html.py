from django.utils.safestring import mark_safe


def test_html(text):
    return mark_safe(text)
