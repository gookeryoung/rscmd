import json
import subprocess
import sys
from pathlib import Path

import pytest

import rscmd


def test_version_matches_package() -> None:
    assert rscmd.__version__ == "0.1.0"


def test_tools_contains_p0_tools() -> None:
    names = {tool["name"] for tool in rscmd.tools()}
    assert {
        "case",
        "wc",
        "json_fmt",
        "hash",
        "codec",
        "regex",
        "path",
        "which",
        "filesearch",
        "stat",
        "csv",
        "timestamp",
        "urlcheck",
    } <= names


def test_invoke_urlcheck_rejects_unsupported_scheme() -> None:
    with pytest.raises(rscmd.RscmdError) as info:
        rscmd.invoke("urlcheck", {"url": "ftp://example.com"})
    assert info.value.code == "UNSUPPORTED_SCHEME"
    assert info.value.field == "url"


def test_invoke_timestamp() -> None:
    out = rscmd.invoke("timestamp", {"input": "2024-02-28T12:00:00Z", "add": "1d"})
    assert out["rfc3339"] == "2024-02-29T12:00:00Z"
    assert out["offset"] == "+00:00"


def test_invoke_csv(tmp_path: Path) -> None:
    path = tmp_path / "data.csv"
    path.write_text("a,b\n1,2\n", encoding="utf-8")
    out = rscmd.invoke("csv", {"input": str(path)})
    assert out["count"] == 1
    assert out["rows"][0]["b"] == "2"


def test_invoke_stat() -> None:
    out = rscmd.invoke("stat", {"values": [1, 2, 3, 4], "percentiles": [90]})
    assert out["count"] == 4
    assert out["median"] == 2.5
    assert abs(out["percentiles"][0]["value"] - 3.7) < 1e-9


def test_invoke_filesearch_finds_name(tmp_path: Path) -> None:
    (tmp_path / "a.txt").write_text("needle\n", encoding="utf-8")
    out = rscmd.invoke("filesearch", {"root": str(tmp_path), "name": "*.txt"})
    assert out["matched"] == 1
    assert out["results"][0]["path"] == "a.txt"


def test_invoke_codec_decodes_base64() -> None:
    out = rscmd.invoke("codec", {"text": "aGVsbG8=", "mode": "base64", "decode": True})
    assert out["text"] == "hello"
    assert out["bytes"] == 5


def test_invoke_regex_returns_matches() -> None:
    out = rscmd.invoke("regex", {"pattern": "(\\d+)", "text": "a1b22"})
    assert out["count"] == 2
    assert out["matches"][1]["text"] == "22"


def test_invoke_path_normalizes() -> None:
    out = rscmd.invoke("path", {"op": "normalize", "path": "a\\b\\..\\c"})
    assert out["path"] == "a/c"


def test_invoke_which_returns_null_for_missing() -> None:
    out = rscmd.invoke("which", {"program": "rscmd-definitely-missing-xyz"})
    assert out["path"] is None


def test_invoke_hash_returns_sha256() -> None:
    out = rscmd.invoke("hash", {"text": "abc"})
    assert out["hex"] == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    assert out["bytes"] == 3


def test_invoke_success_returns_data() -> None:
    assert rscmd.invoke("case", {"text": "HelloWorld", "mode": "snake"}) == {
        "text": "hello_world"
    }


def test_invoke_error_maps_fields() -> None:
    with pytest.raises(rscmd.RscmdError) as info:
        rscmd.invoke("json_fmt", {"json": "{"})

    assert info.value.code == "INVALID_JSON"
    assert info.value.kind == "invalid_input"
    assert info.value.exit_code == 3
    assert info.value.field == "json"


def test_invoke_unknown_tool_raises() -> None:
    with pytest.raises(rscmd.RscmdError) as info:
        rscmd.invoke("nonexistent_tool")
    assert info.value.code == "TOOL_NOT_FOUND"
    assert info.value.exit_code == 4


def test_describe_returns_meta() -> None:
    meta = rscmd.describe("wc")
    assert meta["name"] == "wc"
    assert "summary" in meta
    assert meta["python"] in ("argv_only", "structured")


def test_describe_unknown_tool_raises() -> None:
    with pytest.raises(rscmd.RscmdError) as info:
        rscmd.describe("zzzzzz")
    assert info.value.code == "TOOL_NOT_FOUND"


# --- run_argv / cli_main：新风格入口 ---


def test_run_argv_returns_envelope() -> None:
    envelope = rscmd.run_argv(["case", "--mode", "snake", "HelloWorld"])
    assert envelope["schema"] == "rscmd-result/1"
    assert envelope["ok"] is True
    assert envelope["data"]["text"] == "hello_world"


def test_run_argv_run_subcommand() -> None:
    """新风格：rscmd run case ..."""
    envelope = rscmd.run_argv(["run", "case", "--mode", "snake", "HelloWorld"])
    assert envelope["ok"] is True
    assert envelope["data"]["text"] == "hello_world"


def test_run_argv_run_short_alias() -> None:
    """r 是 run 的短别名"""
    envelope = rscmd.run_argv(["r", "case", "--mode", "snake", "HelloWorld"])
    assert envelope["ok"] is True


def test_run_argv_tools_returns_version_envelope() -> None:
    """`rscmd tools` 的 data 是 rscmd-tools/1 信封，不再是裸数组（breaking 变更）。"""
    envelope = rscmd.run_argv(["tools"])
    data = envelope["data"]
    assert data["schema"] == "rscmd-tools/1"
    assert data["version"] == rscmd.__version__
    assert isinstance(data["tools"], list)
    assert all("aliases" in tool and "side_effects" in tool for tool in data["tools"])


def test_run_argv_run_without_tool_is_error() -> None:
    """rscmd run 不带工具名 → USAGE / MISSING_TOOL"""
    with pytest.raises(rscmd.RscmdError) as info:
        rscmd.run_argv(["run"])
    assert info.value.code == "MISSING_TOOL"
    assert info.value.exit_code == 2


def test_run_argv_version_short_alias() -> None:
    envelope = rscmd.run_argv(["v"])
    assert envelope["ok"] is True
    assert envelope["data"]["version"].startswith("0.1")


def test_run_argv_doctor_short_alias() -> None:
    envelope = rscmd.run_argv(["d"])
    assert envelope["ok"] is True
    assert envelope["data"]["os"] in ("windows", "linux", "macos", "darwin")


def test_run_argv_info_short_alias() -> None:
    envelope = rscmd.run_argv(["i", "wc"])
    assert envelope["ok"] is True
    assert envelope["data"]["name"] == "wc"


def test_cli_main_no_args_returns_help() -> None:
    """无参数 → exit=0 + 帮助文本"""
    exit_code, stdout, stderr = rscmd.cli_main([])
    assert exit_code == 0
    assert "rscmd" in stdout.lower() or "usage" in stdout.lower() or "tool" in stdout.lower()


def test_cli_main_unknown_tool_is_usage() -> None:
    exit_code, stdout, stderr = rscmd.cli_main(["nope"])
    assert exit_code == 2  # USAGE


def test_cli_main_json_fmt_success() -> None:
    exit_code, stdout, _ = rscmd.cli_main(
        ["--json", "json_fmt", "{\"a\": 1}"]
    )
    assert exit_code == 0
    envelope = json.loads(stdout)
    assert envelope["schema"] == "rscmd-result/1"
    assert envelope["ok"] is True
    # CLI 入口返回的是 { "json": "<formatted string>" }
    assert '"a"' in envelope["data"]["json"]


# --- subprocess：端到端 Python console script ---


def test_module_cli_success_json() -> None:
    result = subprocess.run(
        [sys.executable, "-m", "rscmd", "--json", "case", "--mode", "snake", "HelloWorld"],
        check=False,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, result.stderr
    envelope = json.loads(result.stdout)
    assert envelope["schema"] == "rscmd-result/1"
    assert envelope["data"]["text"] == "hello_world"


def test_module_cli_error_json() -> None:
    result = subprocess.run(
        [sys.executable, "-m", "rscmd", "--json", "json_fmt", "{"],
        check=False,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 3
    envelope = json.loads(result.stderr)
    assert envelope["schema"] == "rscmd-error/1"
    assert envelope["code"] == "INVALID_JSON"
    assert envelope["exit_code"] == 3


def test_module_cli_run_subcommand() -> None:
    """子进程新风格：python -m rscmd run case ..."""
    result = subprocess.run(
        [sys.executable, "-m", "rscmd", "--json", "run", "case", "--mode", "snake", "Hi"],
        check=False,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, result.stderr
    envelope = json.loads(result.stdout)
    assert envelope["data"]["text"] == "hi"


def test_module_cli_no_args_shows_help() -> None:
    result = subprocess.run(
        [sys.executable, "-m", "rscmd"],
        check=False,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0
    output = (result.stdout + result.stderr).lower()
    assert "rscmd" in output
