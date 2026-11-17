"""Converts a day's invoices into one currency.

The rates come from a service, one request per currency pair. The service
here is a stand-in that waits as long as a request over a network would;
nothing leaves the machine. Everything else is the real code path.
"""

import time

REQUEST_SECONDS = 0.03


def fetch_rate(pair):
    """Stand-in for an HTTP request to a rates service."""
    time.sleep(REQUEST_SECONDS)
    base, quote = pair
    return 1.0 + (sum(map(ord, base + quote)) % 97) / 100


def convert(invoices, target):
    rates = {}
    total = 0.0
    for currency, amount in invoices:
        if currency == target:
            total += amount
            continue
        if currency not in rates:
            rates[currency] = fetch_rate((currency, target))
        total += amount * rates[currency]
    return round(total, 2)
