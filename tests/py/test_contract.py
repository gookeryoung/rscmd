import json
import subprocess
import sys

import pytest

import rscmd


def test_version_matches_package() -> None:
    assert rscmd.__version__ == "0.1.0"


def test_tools_contains_p0_tools() -> None:
    names = {tool["name"] for tool in rscmd.tools()}
    assert {"case", "wc", "json_fmt"} <= names


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


def test_run_argv_returns_envelope() -> None:
    envelope = rscmd.run_argv(["case", "--mode", "snake", "HelloWorld"])
    assert envelope["schema"] == "rscmd-result/1"
    assert envelope["ok"] is True
    assert envelope["data"]["text"] == "hello_world"


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
