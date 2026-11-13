import helpers as DES
import helpers as hashlib
import helpers as pickle
import helpers as requests
from helpers import Gateway, eval, system


def repository_symbols(command, expression, name):
    system(command)
    eval(expression)
    pickle.loads(command)
    gateway = Gateway()
    gateway.execute("SELECT * FROM users WHERE name = '" + name + "'")
    hashlib.md5(command)
    DES.new(command)
    requests.get(command, verify=False)
