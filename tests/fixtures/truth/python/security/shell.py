import subprocess


def shell_commands(command):
    subprocess.run(["sh", "-c", command])
    subprocess.run(["ls", "-c", command])
