from datetime import datetime

from pydantic import BaseModel, Field


class UserBase(BaseModel):
    email: str
    full_name: str


class UserCreate(UserBase):
    password: str = Field(min_length=12)


class UserRead(UserBase):
    id: int
    created_at: datetime = Field(serialization_alias="createdAt")
    is_active: bool = True
