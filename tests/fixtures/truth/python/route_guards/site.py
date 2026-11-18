from flask import Flask
from flask_login import login_required

site = Flask(__name__)


@login_required
@site.route("/unprotected")
def unprotected():
    return "open"


@site.route("/protected/<name>")
@login_required
def protected(name):
    return name
