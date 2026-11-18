from typing import Annotated

from fastapi import APIRouter, BackgroundTasks, FastAPI, Query, Request, Security
from fastapi import Depends
from fastapi import Depends as Needs

from .auth import count_request, get_db, verify_admin, verify_key, verify_region, verify_scope
from .models import Item, User

app = FastAPI(dependencies=[Depends(count_request)])
router = APIRouter(prefix="/v1", dependencies=[Depends(verify_key)])
open_router = APIRouter()
CurrentUser = Annotated[User, Depends(verify_key)]


@router.post(
    "/items/{item_id}",
    dependencies=[Needs(verify_admin), Security(verify_scope, scopes=["items"])],
)
async def update(
    item_id: int,
    item: Item,
    request: Request,
    tasks: BackgroundTasks,
    limit: int = Query(10),
    session=Depends(get_db),
    user: User = Depends(verify_key),
):
    return item


@router.get("/me")
async def me(user: CurrentUser, session: Annotated[object, Depends(get_db)]):
    return user


# dependencies=[Depends(verify_key)]
@open_router.get("/health", description="dependencies=[Depends(verify_key)]")
async def health(verbose: bool = False, note: str = "Depends(verify_key)"):
    return verbose


app.include_router(router, dependencies=[Depends(verify_region)])


@router.get("/after")
async def after():
    return 1


@app.get("/late")
async def late():
    return 2
