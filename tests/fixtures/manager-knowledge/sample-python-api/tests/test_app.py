from app import health


def test_health_is_ok():
    assert health() == {"status": "ok"}
