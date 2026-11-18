from django.urls import path

from . import views

urlpatterns = [
    path("items/<int:pk>/", views.detail),
    path("landing/", views.landing),
]
