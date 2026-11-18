from fastapi import APIRouter

from .auth import verify_key

decoy_router = APIRouter()


def Depends(function):
    return function


@decoy_router.get("/local", dependencies=[Depends(verify_key)])
async def local(user=Depends(verify_key)):
    return user
