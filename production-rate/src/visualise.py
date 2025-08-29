import sys
import time
import click
import requests
import streamlit as st
import pandas as pd
import numpy as np
from prometheus_client.parser import text_string_to_metric_families
from collections import deque

METRIC_NAME = "experiment_producer_event_count_total"
SCRAPE_INTERVAL = 1
MAX_POINTS = 60*5

def scrape(uri: str):
    try:
        resp = requests.get(uri, timeout=5)
        resp.raise_for_status()
        for family in text_string_to_metric_families(resp.text):
            if family.name == METRIC_NAME:
                for sample in family.samples:
                    ts = time.time()
                    val = sample.value
                    st.session_state["timestamps"].append(pd.to_datetime(ts, unit="s"))
                    st.session_state["values"].append(val)
                    break
    except Exception as e:
        print(f"Error scraping metrics: {e}")

@click.command()
@click.option('--producer-connection', 'producer_connection', default="localhost:3001")
def main(producer_connection: str):
    uri = f"http://{producer_connection}/metrics"
    st.session_state["timestamps"] = deque(maxlen=MAX_POINTS)
    st.session_state["values"] = deque(maxlen=MAX_POINTS)

    st.title("Producer Metrics (events/s)")
    st.write(f"Scraping `{METRIC_NAME}` from `{uri}` every {SCRAPE_INTERVAL}s")
    placeholder = st.empty()

    while True:
        scrape(uri)
        timestamps, values = st.session_state["timestamps"], st.session_state["values"]
        timediff = (pd.Series(timestamps).astype(np.int64) - pd.Series(timestamps).shift().astype(np.int64)) / 1e9
        eventdiff = pd.Series(values) - pd.Series(values).shift()

        with placeholder.container():
            df = pd.DataFrame({
                "Timestamp": list(st.session_state["timestamps"]),
                "Production Rate (events/s)": eventdiff / timediff
            })
            st.line_chart(df, x="Timestamp", y="Production Rate (events/s)")
        current_time = time.time()
        sleep_time = int(current_time + SCRAPE_INTERVAL) - current_time
        time.sleep(sleep_time)

main()
