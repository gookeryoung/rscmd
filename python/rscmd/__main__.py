"""console script 入口：进程内调用原生 CLI，不起子进程。"""

from __future__ import annotations

import sys

from . import _native

_MCP_USAGE = (
    "用法: rscmd mcp\n\n"
    "以 MCP stdio server 模式运行：从 stdin 逐行读取 JSON-RPC 请求，向 stdout 写响应。"
)


def main(argv: list[str] | None = None) -> int:
    args = list(argv if argv is not None else sys.argv[1:])
    # MCP 是阻塞式 stdio 服务，交给原生模块直接接管 stdin/stdout。
    if args[:1] == ["mcp"]:
        if len(args) > 1:
            sys.stderr.write(f"{_MCP_USAGE}\n")
            return 2
        sys.stdout.flush()
        sys.stderr.flush()
        return _native.mcp_serve()
    code, stdout, stderr = _native.cli_main(args)
    if stdout:
        sys.stdout.write(stdout)
    if stderr:
        sys.stderr.write(stderr)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
