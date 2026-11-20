def run(items):
    chosen = filter(None, items)
    first = next(iter(items))
    return type(first), chosen
