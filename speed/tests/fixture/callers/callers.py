"""The fixture's caller targets: each calls the library, which reaches the
compared function."""

import os

import library


def two_naps():
    # nap(2) twice: the case nap-2.
    return library.naps([2, 2])


def a_nap_no_case_has():
    return library.naps([3])


def naps_that_change():
    # nap(2), then nap(0) on the next call: each a case, not the same.
    library.calls[0] += 1
    return library.naps([2 if library.calls[0] % 2 else 0])


def a_nap_set_by_the_environment():
    # nap(2), or the nap a variable names: the same within a process, not
    # between the build's and the run's.
    return library.naps([int(os.environ.get("SPEED_FIXTURE_NAP", "2"))])


def a_set_of_letters():
    # A set of strings, whose order follows the hash seed: the case
    # size-letters, or the letters a variable names.
    return library.sized(set(os.environ.get("SPEED_FIXTURE_LETTERS", "abcdefgh")))


def a_wrong():
    # wrong(1): the case wrong-1, whose output the Python port gets wrong.
    return library.call_wrong(1)
