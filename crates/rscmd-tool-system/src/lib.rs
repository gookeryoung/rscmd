//! 系统域工具。

pub mod archive;
pub mod env;
pub mod filedate;
pub mod filesearch;
pub mod path;
pub mod reshack;
pub mod which;

use std::sync::OnceLock;

use rscmd_core::ToolDef;

pub fn tools() -> &'static [ToolDef] {
    static TOOLS: OnceLock<Vec<ToolDef>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        vec![
            archive::def(),
            env::def(),
            filedate::def(),
            filesearch::def(),
            path::def(),
            reshack::def(),
            which::def(),
        ]
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn exposes_every_domain_tool_once() {
        let first = super::tools();
        let second = super::tools();
        assert_eq!(first.len(), 7);
        assert!(std::ptr::eq(first, second), "应复用同一份缓存");
    }
}
