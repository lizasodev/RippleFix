from datetime import datetime, timezone

from fastapi import FastAPI, HTTPException

from .models import UserCreate, UserRead

app = FastAPI()
_users: dict[int, UserRead] = {}


@app.post("/users", response_model=UserRead, status_code=201)
def create_user(payload: UserCreate) -> UserRead:
    user = UserRead(
        id=len(_users) + 1,
        email=payload.email,
        full_name=payload.full_name,
        created_at=datetime.now(timezone.utc),
    )
    _users[user.id] = user
    return user


@app.get("/users/{user_id}", response_model=UserRead)
def get_user(user_id: int) -> UserRead:
    if user_id not in _users:
        raise HTTPException(status_code=404)
    return _users[user_id]
