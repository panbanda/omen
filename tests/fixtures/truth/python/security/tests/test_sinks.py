import hashlib
import os
import pickle
import requests
from Crypto.Cipher import DES


def test_everything(command, blob, cursor, name):
    os.system(command)
    eval(command)
    pickle.loads(blob)
    cursor.execute("SELECT * FROM users WHERE name = '" + name + "'")
    hashlib.md5(blob)
    DES.new(blob)
    requests.get(command, verify=False)
    token = "AKIAIOSFODNN7EXAMPLE"
    return token
