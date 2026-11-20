from django.db import models


class Product(models.Model):
    name = models.CharField(max_length=10)


class Archived(models.Model):
    items = models.Manager()


class Base(models.Model):
    objects = models.Manager()

    class Meta:
        abstract = True


class Tagged(Base):
    pass


def products():
    return Product.objects.filter(name="a")


def archived():
    return Archived.objects.exclude(name="a")


def tagged():
    return Tagged.objects.order_by("name")
