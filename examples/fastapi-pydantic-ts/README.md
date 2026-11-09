# Example: FastAPI + Pydantic + TypeScript

Reference fixture for RippleFix's first vertical slice. It is used by the CLI
end-to-end tests (`crates/ripplefix-cli/tests/analyze.rs`).

- `backend/app/models.py`: Pydantic models (`UserBase`, `UserCreate`, `UserRead`)
- `backend/app/main.py`: FastAPI routes that use `UserRead` as a `response_model`
- `backend/tests/`: API tests that depend on the `full_name` JSON key
- `frontend/src/api.ts`: TypeScript consumer of the `UserRead` contract

Try it:

```sh
cp -r examples/fastapi-pydantic-ts /tmp/demo && cd /tmp/demo
git init -q && git add . && git commit -qm init
sed -i 's/full_name: str/display_name: str/' backend/app/models.py
ripplefix analyze
```

Today RippleFix reports the inferred rename of `UserBase.full_name`, with its
evidence. Tracing the change to the FastAPI routes, the TypeScript
interface, and the tests is the next milestone (see `docs/roadmap.md`).
