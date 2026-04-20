use emmylua_parser::LuaChunk;

use crate::FileId;

use super::super::{
    SalsaDeclTreeSummary, SalsaGlobalSummary, SalsaMemberIndexSummary, SalsaModuleSummary,
    summary::build_module_summary,
};

pub fn analyze_module_summary(
    file_id: FileId,
    decl_tree: &SalsaDeclTreeSummary,
    globals: &SalsaGlobalSummary,
    members: &SalsaMemberIndexSummary,
    chunk: LuaChunk,
) -> Option<SalsaModuleSummary> {
    build_module_summary(file_id, decl_tree, globals, members, chunk)
}
