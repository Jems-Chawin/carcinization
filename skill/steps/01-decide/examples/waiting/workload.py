"""The workload measured for the decision: one day's invoices."""

import exchange_rates

CURRENCIES = ["USD", "GBP", "JPY", "CHF", "SEK", "NOK", "DKK", "PLN", "CZK", "HUF",
              "CAD", "AUD", "NZD", "SGD", "HKD", "KRW", "INR", "BRL", "MXN", "ZAR"]


def run(invoices):
    """`invoices` invoices spread over twenty currencies, converted to EUR."""
    day = [(CURRENCIES[i % len(CURRENCIES)], 10.0 + i % 500) for i in range(invoices)]
    return exchange_rates.convert(day, "EUR")
