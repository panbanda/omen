import os
from flask import Flask
from flask_login import login_required

app = Flask(__name__)


@app.route("/run", methods=["POST"])
@login_required
def run_command():
    return prepare()


def prepare():
    return execute()


def execute():
    os.system("x" + os.environ["A"])


def orphan(expression):
    eval(expression)
