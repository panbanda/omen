import unittest

import pytest


class _Size:
    def __init__(self, value):
        self.value = value

    def __eq__(self, other):
        return self._live_from_special_method(other)

    def _live_from_special_method(self, other):
        return self.value == other.value

    def _dead_in_class(self):
        return self.value


@pytest.fixture
def _database():
    return _live_from_fixture()


def _live_from_fixture():
    return _Size(1)


def _test_like_name_in_production():
    return _Size(2)


class _ServiceCheck(unittest.TestCase):
    def test_sizes(self):
        self.assertEqual(self._live_helper_of_a_test(), _Size(1))

    def _live_helper_of_a_test(self):
        return _Size(1)
