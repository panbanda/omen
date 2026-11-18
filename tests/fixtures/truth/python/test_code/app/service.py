import unittest

import pytest


def production():
    return 1


def test_connection():
    return production()


class TestRegistry:
    def test_entry(self):
        return production()


@pytest.fixture
def database():
    return production()


class ServiceCheck(unittest.TestCase):
    def setUp(self):
        self.value = production()

    def test_runs(self):
        self.assertEqual(self.helper(), 1)

    def helper(self):
        return self.value
