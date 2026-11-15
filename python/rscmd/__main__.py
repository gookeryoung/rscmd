"""console script 入口：进程内调用原生 CLI，不起子进程。"""

from __future__ import annotations

import sys

from . import _native


def main(argv: list[str] | None = None) -> int:
    code, stdout, stderr = _native.cli_main(list(argv if argv is not None else sys.argv[1:]))
    if stdout:
        sys.stdout.write(stdout)
    if stderr:
        sys.stderr.write(stderr)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
