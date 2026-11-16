//! 工具契约、注册表、统一信封与调度。
//!
//! 本 crate 不依赖任何具体工具或基础件，只依赖标准库与少量契约依赖。

pub mod args;
pub mod builtins;
pub mod ctx;
pub mod dispatch;
pub mod envelope;
pub mod errors;
pub mod registry;
pub mod render;
pub mod tool;

pub use args::{global_schema, GlobalArgs};
pub use ctx::ToolCtx;
pub use dispatch::{dispatch, Outcome};
pub use errors::{ErrorKind, RscmdError, ERROR_CODE_KINDS};
pub use registry::Registry;
pub use render::render;
pub use tool::{
    command_for, meta_json, meta_schema, PythonExposure, SideEffects, ToolDef, ToolMeta,
};
