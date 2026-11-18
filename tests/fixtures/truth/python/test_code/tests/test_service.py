from app.service import production


def helper():
    return production()


def test_runs():
    assert helper() == 1


class TestService:
    def test_method(self):
        assert helper() == 1
