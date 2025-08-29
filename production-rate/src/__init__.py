import os
import sys
import subprocess

def start():
    visualise = os.path.join(os.path.dirname(__file__), 'visualise.py')
    command = ["streamlit", "run", "--server.address", "0.0.0.0", visualise, "--"]
    args = sys.argv[1:]
    command.extend(args)
    subprocess.run(command)
