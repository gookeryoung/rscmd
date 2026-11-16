//! 结构化数据域工具。

pub mod csv;
pub mod json_fmt;
pub mod stat;
pub mod timestamp;

use std::sync::OnceLock;

use rscmd_core::ToolDef;

pub fn tools() -> &'static [ToolDef] {
    static TOOLS: OnceLock<Vec<ToolDef>> = OnceLock::new();
    TOOLS.get_or_init(|| vec![csv::def(), json_fmt::def(), stat::def(), timestamp::def()])
}

#[cfg(test)]
mod tests {
    #[test]
    fn exposes_domain_tools() {
        assert_eq!(super::tools().len(), 4);
        assert_eq!(super::tools()[0].meta.name, "csv");
        assert_eq!(super::tools()[1].meta.name, "json_fmt");
        assert_eq!(super::tools()[2].meta.name, "stat");
        assert_eq!(super::tools()[3].meta.name, "timestamp");
    }
}
