"""The piece of Python the recorded tests look at."""
import json
import os.path

import yaml
import zeep
from google import genai
from opentelemetry import trace

from . import sibling

tracer = trace.get_tracer(__name__)


def run(path):
    with tracer.start_as_current_span("run"):
        config = yaml.safe_load(open(path))
        client = zeep.Client(wsdl=config["wsdl"])
        return json.dumps(genai.Client().models.list()), client.service.Ping(), os.path.basename(path)
