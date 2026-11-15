import json as js
import os

import requests

from util import max


def helper():
    return 1


def run(items):
    size = len(items)
    os.path.join("a", "b")
    js.dumps({})
    requests.get("u")
    helper()
    max(1, 2)
    print(size)


def shadowed(len):
    return len(3)
