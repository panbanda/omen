import logging
from collections import defaultdict

from app.collections import defaultdict as local_default


def run():
    counts = defaultdict(int)
    logger = logging.getLogger("app")
    logger.info("run")
    return counts, local_default(int)
