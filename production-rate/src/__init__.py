import os
import subprocess

def start():
    visualise = os.path.join(os.path.dirname(__file__), 'visualise.py')
    subprocess.run(["streamlit", "run", visualise, "--", "--producer-connection", "producer:3001"])
