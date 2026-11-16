"""rscmd 的 Python 绑定：只做类型转换与异常映射，业务逻辑全部在 Rust 侧。"""

from __future__ import annotations

import json as _json
from typing import Any, Mapping, NoReturn, Sequence

from . import _native

__all__ = [
    "RscmdError",
    "__version__",
    "cli_main",
    "describe",
    "invoke",
    "run_argv",
    "tools",
]

__version__: str = _native.version()


class RscmdError(Exception):
    """与 Rust 侧 RscmdError 字段一一对应的异常。"""

    def __init__(self, payload: Mapping[str, Any]) -> None:
        self.kind = payload.get("kind")
        self.code = payload.get("code")
        self.exit_code = payload.get("exit_code")
        self.field = payload.get("field")
        self.expected = payload.get("expected")
        self.hint = payload.get("hint")
        super().__init__(payload.get("message") or "rscmd error")


def _raise_native(error: BaseException) -> NoReturn:
    try:
        payload = _json.loads(str(error))
    except Exception:  # noqa: BLE001 - 保底：原生错误不是 JSON 时仍可读
        payload = {"message": str(error)}
    raise RscmdError(payload) from None


def tools() -> list[dict[str, Any]]:
    """返回全部工具的摘要元数据。"""
    return _native.tools()


def describe(name: str) -> dict[str, Any]:
    """返回单个工具的完整元数据。"""
    try:
        return _native.describe(name)
    except _native.NativeError as error:
        _raise_native(error)


def invoke(tool: str, args: Mapping[str, Any] | None = None) -> Any:
    """以结构化参数调用工具；失败时抛出 RscmdError。"""
    try:
        return _native.invoke(tool, dict(args or {}))
    except _native.NativeError as error:
        _raise_native(error)


def run_argv(argv: Sequence[str]) -> dict[str, Any]:
    """按 CLI 语义调用工具，返回成功信封。"""
    try:
        return _native.run_argv([str(item) for item in argv])
    except _native.NativeError as error:
        _raise_native(error)


def cli_main(argv: Sequence[str]) -> tuple[int, str, str]:
    """CLI 级入口：返回 (exit_code, stdout, stderr)。

    与 `python -m rscmd` 的行为一致；未知命令、解析失败都会以 exit_code=2/3/4 区分。
    """
    return _native.cli_main([str(item) for item in argv])
