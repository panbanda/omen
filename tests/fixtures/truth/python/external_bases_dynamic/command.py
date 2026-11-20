from django.core.management.base import BaseCommand


class Command(BaseCommand):
    def __init__(self, *args):
        super().__init__(*args)


def install(target, name, value):
    setattr(target, name, value)
