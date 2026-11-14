import yaml
from yaml import SafeLoader as Chosen
from yaml import FullLoader as Chosen


def rebound(text):
    yaml.load(text, Loader=Chosen)
