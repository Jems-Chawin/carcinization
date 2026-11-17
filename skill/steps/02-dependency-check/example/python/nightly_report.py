"""Nightly job: fetch supplier price lists and flag odd prices.

The piece being considered for a port. It is small, and it leans on four
libraries that fare very differently on the Rust side.
"""

import json
import logging
from pathlib import Path

import yaml
import zeep
from opentelemetry import trace
from sklearn.ensemble import IsolationForest

log = logging.getLogger(__name__)
tracer = trace.get_tracer(__name__)


def load_config(path):
    with open(path, encoding="utf-8") as f:
        config = yaml.safe_load(f)
    if "suppliers" not in config:
        raise ValueError("config has no suppliers")
    return config


def fetch_prices(wsdl_url, supplier_id):
    """Every (item, price) pair a supplier's SOAP service lists."""
    client = zeep.Client(wsdl=wsdl_url)
    reply = client.service.GetPriceList(supplierId=supplier_id)
    return [(line.item, float(line.price)) for line in reply.lines]


def odd_prices(rows, contamination):
    """Rows whose price an isolation forest marks as an outlier."""
    if len(rows) < 8:
        return []
    model = IsolationForest(contamination=contamination, random_state=0)
    marks = model.fit_predict([[price] for _, price in rows])
    return [row for row, mark in zip(rows, marks) if mark == -1]


def run(config_path, out_path):
    config = load_config(config_path)
    report = {}
    for supplier in config["suppliers"]:
        with tracer.start_as_current_span("supplier") as span:
            span.set_attribute("supplier.name", supplier["name"])
            try:
                rows = fetch_prices(supplier["wsdl"], supplier["id"])
                report[supplier["name"]] = odd_prices(rows, config.get("contamination", 0.05))
            except zeep.exceptions.Error as error:  # one bad supplier must not stop the night
                log.warning("skipping %s: %s", supplier["name"], error)
                span.record_exception(error)
    Path(out_path).write_text(json.dumps(report, indent=2), encoding="utf-8")
    return report
