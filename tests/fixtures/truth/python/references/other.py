import os


def cleanup():
    os.system("rm " + os.environ["TARGET"])
