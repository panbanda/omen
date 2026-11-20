class QuerySet:
    def filter(self, **query):
        return self

    def next(self):
        return self


class Block:
    type: str = "paragraph"
