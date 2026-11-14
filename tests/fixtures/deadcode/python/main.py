from other import run


class Tool:
    def live_method(self):
        pass

    def _dead_method(self):
        pass


def _live_direct():
    pass


def _live_value():
    pass


def _dead_plain():
    pass


def _dead_caller():
    _dead_callee()


def _dead_callee():
    pass


def _dead_unresolved():
    pass


def live_public():
    pass


def main():
    _live_direct()
    handler = _live_value
    handler()
    tool: Tool = Tool()
    tool.live_method()
    run(None)


if __name__ == "__main__":
    main()
