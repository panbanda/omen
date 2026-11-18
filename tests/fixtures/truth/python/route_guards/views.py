from django.contrib.auth.decorators import login_required, permission_required
from django.views.decorators.http import require_POST


@login_required
@require_POST
@permission_required("items.change")
def detail(request, pk):
    return pk


# @login_required
def landing(request):
    """@login_required"""
    return 1
