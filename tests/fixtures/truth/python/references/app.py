import os
from flask import Flask

app = Flask(__name__)


def audit(event=None):
    os.system("logger " + os.environ["EVENT"])


@app.route("/jobs", methods=["POST"])
def create_job():
    def load_module():
        eval(os.environ["JOB"])

    schedule(load_module, 5)
    schedule(audit, 1)
    return "ok"


@app.route("/shadow", methods=["POST"])
def shadowed(audit):
    schedule(audit, 1)
    return "ok"


@app.route("/other", methods=["POST"])
def other_file():
    schedule(cleanup, 1)
    return "ok"


@app.route("/text", methods=["POST"])
def text_only():
    # schedule(audit, 1)
    schedule("audit", 1)
    return "ok"


def schedule(task, delay):
    return (task, delay)
