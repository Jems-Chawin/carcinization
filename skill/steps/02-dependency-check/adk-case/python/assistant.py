"""A small agent built on Google's Agent Development Kit and Gemini SDK.

This is the case that started the project: an agent asked to port code like
this said there was no Rust crate for either library.
"""

from google import genai
from google.adk.agents import Agent
from google.adk.runners import Runner


def summarise(text):
    client = genai.Client()
    reply = client.models.generate_content(model="gemini-2.5-flash", contents="Summarise: " + text)
    return reply.text


def build_runner(session_service):
    agent = Agent(name="helper", model="gemini-2.5-flash", instruction="Answer briefly.")
    return Runner(agent=agent, app_name="helper", session_service=session_service)
