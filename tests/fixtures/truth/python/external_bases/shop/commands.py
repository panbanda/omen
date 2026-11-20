from django.core.management.base import BaseCommand


class Command(BaseCommand):
    def __init__(self, *args):
        super().__init__(*args)


class Mixin:
    def handle(self):
        return 1


class Loud(BaseCommand):
    def handle(self):
        return super().handle()


class Combined(Loud, Mixin):
    pass


class Dynamic(BaseCommand):
    def __getattr__(self, name):
        return name

    def execute(self):
        return super().execute()


class Meta(type):
    pass


class Typed(BaseCommand, metaclass=Meta):
    def check(self):
        return super().check()


class Patched(BaseCommand):
    def flush(self):
        return super().flush()


BaseCommand.flush = print
