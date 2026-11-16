//! 工具聚合：全仓唯一知道“有哪些工具域”的地方。

use std::sync::OnceLock;

use rscmd_core::{Registry, ToolDef};

/// 按 feature 汇总所有已启用的工具。首次访问做一次拷贝，之后只读。
pub fn tools() -> &'static [ToolDef] {
    static ALL: OnceLock<Vec<ToolDef>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut tools = Vec::new();
        #[cfg(feature = "text")]
        tools.extend_from_slice(rscmd_tool_text::tools());
        #[cfg(feature = "data")]
        tools.extend_from_slice(rscmd_tool_data::tools());
        #[cfg(feature = "crypto")]
        tools.extend_from_slice(rscmd_tool_crypto::tools());
        #[cfg(feature = "system")]
        tools.extend_from_slice(rscmd_tool_system::tools());
        tools
    })
}

pub fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| Registry::new(tools()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn aggregates_enabled_domains() {
        let names: Vec<&str> = super::tools().iter().map(|tool| tool.meta.name).collect();
        #[cfg(feature = "text")]
        assert!(names.contains(&"case"));
        #[cfg(feature = "data")]
        assert!(names.contains(&"json_fmt"));
        #[cfg(feature = "crypto")]
        assert!(names.contains(&"hash"));
        #[cfg(feature = "system")]
        assert!(names.contains(&"path"));
        #[cfg(feature = "system")]
        assert!(names.contains(&"which"));
        assert!(!names.is_empty());
    }

    #[test]
    fn registry_is_valid_and_findable() {
        assert!(super::registry().validate().is_ok());
        assert_eq!(
            super::registry().find("jsonfmt").unwrap().meta.name,
            "json_fmt"
        );
        assert!(std::ptr::eq(super::registry(), super::registry()));
    }
}
