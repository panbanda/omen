import hashlib
import os
import pickle
import requests
import requests as http
import subprocess as sp
import yaml
from builtins import exec as run_code
from Crypto.Cipher import ARC4 as Stream
from Crypto.Cipher import DES
from hashlib import sha1 as digest_one
from pickle import loads as unpickle


def command_execution(command):
    os.system(command)
    sp.run(command, shell=True)
    sp.run(["ls", "-l"])
    text = "os.system(command)"
    # os.system(command)
    return text


def code_evaluation(expression):
    eval(expression)
    run_code(expression)
    compile(expression, "<string>", "eval")
    text = "eval(expression)"
    # eval(expression)
    return text


def unsafe_deserialization(blob, text):
    pickle.loads(blob)
    unpickle(blob)
    yaml.load(text, Loader=yaml.SafeLoader)
    text = "pickle.loads(blob)"
    # pickle.loads(blob)
    return text


def sql_string_construction(cursor, name):
    cursor.execute("SELECT * FROM users WHERE name = '" + name + "'")
    cursor.execute(f"DELETE FROM users WHERE name = '{name}'")
    cursor.execute("SELECT * FROM users WHERE name = %s", (name,))
    text = "cursor.execute(query)"
    # cursor.execute(query)
    return text


def weak_hash(data):
    hashlib.md5(data)
    digest_one(data)
    hashlib.sha256(data)
    text = "hashlib.md5(data)"
    # hashlib.md5(data)
    return text


def weak_cipher(key):
    DES.new(key, DES.MODE_ECB)
    Stream.new(key)
    hashlib.pbkdf2_hmac("sha256", key, key, 1000)
    text = "DES.new(key)"
    # DES.new(key)
    return text


def tls_verification_disabled(url):
    requests.get(url, verify=False)
    http.post(url, verify=False)
    requests.get(url, verify=True)
    text = "requests.get(url, verify=False)"
    # requests.get(url, verify=False)
    return text


def hardcoded_credential():
    token = "AKIAIOSFODNN7EXAMPLE"
    name = "AKIAEXAMPLE"
    # AKIAIOSFODNN7EXAMPLE
    return token, name
