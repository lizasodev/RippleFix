from fastapi.testclient import TestClient

from app.main import app

client = TestClient(app)


def test_create_user_returns_full_name():
    res = client.post(
        "/users",
        json={"email": "ada@example.com", "full_name": "Ada Lovelace", "password": "correct-horse-battery"},
    )
    assert res.status_code == 201
    assert res.json()["full_name"] == "Ada Lovelace"
