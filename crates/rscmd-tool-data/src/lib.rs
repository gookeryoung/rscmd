//! 结构化数据域工具。

pub mod json_fmt;
pub mod stat;

use std::sync::OnceLock;

use rscmd_core::ToolDef;

pub fn tools() -> &'static [ToolDef] {
    static TOOLS: OnceLock<Vec<ToolDef>> = OnceLock::new();
    TOOLS.get_or_init(|| vec![json_fmt::def(), stat::def()])
}

#[cfg(test)]
mod tests {
    #[test]
    fn exposes_domain_tools() {
        assert_eq!(super::tools().len(), 2);
        assert_eq!(super::tools()[0].meta.name, "json_fmt");
        assert_eq!(super::tools()[1].meta.name, "stat");
    }
}
