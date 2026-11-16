//! Python 绑定：薄封装，业务逻辑全部复用 rscmd-core / rscmd-tools。

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use serde_json::{json, Value};

use rscmd_core::{RscmdError, ToolCtx, ToolDef};

create_exception!(rscmd, NativeError, PyException);

fn find_tool(name: &str) -> Option<&'static ToolDef> {
    rscmd_tools::tools()
        .iter()
        .find(|tool| tool.meta.name == name || tool.meta.aliases.contains(&name))
}

fn native_error(err: &RscmdError) -> PyErr {
    let payload = json!({
        "kind": err.kind.as_str(),
        "code": err.code,
        "message": err.message,
        "field": err.field,
        "expected": err.expected,
        "hint": err.hint,
        "exit_code": err.exit_code(),
    });
    NativeError::new_err(payload.to_string())
}

fn missing_tool(name: &str) -> PyErr {
    native_error(
        &RscmdError::coded("TOOL_NOT_FOUND", format!("未知工具: {name}"))
            .with_hint("使用 tools() 查看可用工具"),
    )
}

#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[pyfunction]
fn tools(py: Python<'_>) -> PyResult<PyObject> {
    let items: Vec<Value> = rscmd_tools::tools()
        .iter()
        .map(rscmd_core::tool::meta_json)
        .collect();
    Ok(pythonize::pythonize(py, &items)?.unbind())
}

#[pyfunction]
fn describe(py: Python<'_>, name: &str) -> PyResult<PyObject> {
    let tool = find_tool(name).ok_or_else(|| missing_tool(name))?;
    Ok(pythonize::pythonize(py, &rscmd_core::tool::meta_json(tool))?.unbind())
}

#[pyfunction]
#[pyo3(signature = (tool, args=None))]
fn invoke(py: Python<'_>, tool: &str, args: Option<&Bound<'_, PyAny>>) -> PyResult<PyObject> {
    let def = find_tool(tool).ok_or_else(|| missing_tool(tool))?;
    let run = def.run_json.ok_or_else(|| {
        PyValueError::new_err(format!("工具 `{tool}` 未声明 Structured Python 入口"))
    })?;

    let value: Value = match args {
        Some(obj) => pythonize::depythonize(obj)
            .map_err(|err| PyValueError::new_err(format!("参数无法转换为 JSON: {err}")))?,
        None => json!({}),
    };

    let mut ctx = ToolCtx::default();
    let output = run(&mut ctx, &value).map_err(|err| native_error(&err))?;
    Ok(pythonize::pythonize(py, &output)?.unbind())
}

#[pyfunction]
fn run_argv(py: Python<'_>, argv: Vec<String>) -> PyResult<PyObject> {
    let outcome = rscmd_core::dispatch(rscmd_tools::tools(), &argv);
    match &outcome.result {
        Ok(data) => {
            let ok = !rscmd_core::builtins::check_failed(&outcome.tool, data);
            let envelope =
                rscmd_core::envelope::result(&outcome.tool, outcome.subcommand(), data.clone(), ok);
            Ok(pythonize::pythonize(py, &envelope)?.unbind())
        }
        Err(err) => Err(native_error(err)),
    }
}

/// 供 Python console script 使用：返回 (exit_code, stdout, stderr)。
#[pyfunction]
fn cli_main(argv: Vec<String>) -> (i32, String, String) {
    let outcome = rscmd_core::dispatch(rscmd_tools::tools(), &argv);
    let (stdout, stderr) = rscmd_core::render(&outcome);
    (outcome.exit_code(), stdout, stderr)
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(version, module)?)?;
    module.add_function(wrap_pyfunction!(tools, module)?)?;
    module.add_function(wrap_pyfunction!(describe, module)?)?;
    module.add_function(wrap_pyfunction!(invoke, module)?)?;
    module.add_function(wrap_pyfunction!(run_argv, module)?)?;
    module.add_function(wrap_pyfunction!(cli_main, module)?)?;
    module.add("NativeError", module.py().get_type::<NativeError>())?;
    Ok(())
}
