import requests
import subprocess
import yaml
from yaml import FullLoader as Trusting
from yaml import SafeLoader as Careful


def nested_values(url, text, cursor, name):
    subprocess.run("ls", env={"shell": True})
    requests.post(url, json={"verify": False})
    cursor.execute(reformat(name))
    yaml.load(text, Loader=Careful)
    yaml.load(text, Loader=Trusting)
    yaml.load(text)
    yaml.load(text, Loader=custom_loader)
