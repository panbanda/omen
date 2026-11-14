import yaml
from yaml import FullLoader as Chosen
from yaml import SafeLoader as Chosen


def rebound(text):
    yaml.load(text, Loader=Chosen)
