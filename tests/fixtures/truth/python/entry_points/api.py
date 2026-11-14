from fastapi import FastAPI

api = FastAPI()


@api.get("/items/{item_id}")
async def read(item_id: int):
    return item_id
