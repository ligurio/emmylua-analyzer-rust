use emmylua_parser::{LuaAstNode, LuaChunk, LuaExpr};
use smol_str::SmolStr;

use crate::{
    FileId,
    compilation::summary_builder::query::{
        SalsaModuleResolveIndex, build_module_resolve_index, resolve_module_export_in_index,
    },
};

use super::super::{
    SalsaDeclId, SalsaDeclTreeSummary, SalsaGlobalFunctionSummary, SalsaGlobalSummary,
    SalsaGlobalVariableSummary, SalsaMemberIndexSummary, SalsaMemberPathRootSummary,
    SalsaMemberPathSummary, SalsaMemberSummary, analysis::analyze_module_return_points,
};

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct SalsaModuleSummary {
    pub file_id: u32,
    pub export_target: Option<SalsaExportTargetSummary>,
    pub export: Option<SalsaModuleExportSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub enum SalsaExportTargetSummary {
    LocalName(SmolStr),
    Member(SalsaMemberPathSummary),
    Closure(u32),
    Table(u32),
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub enum SalsaModuleExportSummary {
    LocalDecl { name: SmolStr, decl_id: SalsaDeclId },
    Member(SalsaMemberSummary),
    GlobalVariable(SalsaGlobalVariableSummary),
    GlobalFunction(SalsaGlobalFunctionSummary),
    Closure { signature_offset: u32 },
    Table { table_offset: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub enum SalsaModuleExportResolveStateSummary {
    Partial,
    Resolved,
    RecursiveDependency,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct SalsaModuleExportQuerySummary {
    pub export_target: SalsaExportTargetSummary,
    pub export: Option<SalsaModuleExportSummary>,
    pub semantic_target: Option<crate::SalsaSemanticTargetSummary>,
    pub doc_owners: Vec<crate::SalsaDocOwnerResolveSummary>,
    pub tag_properties: Vec<crate::SalsaDocTagPropertySummary>,
    pub properties: Vec<crate::SalsaPropertySummary>,
    pub state: SalsaModuleExportResolveStateSummary,
}

pub fn build_module_summary(
    file_id: FileId,
    decl_tree: &SalsaDeclTreeSummary,
    globals: &SalsaGlobalSummary,
    members: &SalsaMemberIndexSummary,
    chunk: LuaChunk,
) -> Option<SalsaModuleSummary> {
    let resolve_index = build_module_resolve_index(decl_tree, globals, members);
    build_module_summary_with_index(file_id, &resolve_index, chunk)
}

pub fn build_module_summary_with_index(
    file_id: FileId,
    resolve_index: &SalsaModuleResolveIndex,
    chunk: LuaChunk,
) -> Option<SalsaModuleSummary> {
    let export_target = analyze_module_return_points(chunk.get_block()?)
        .into_iter()
        .find_map(summarize_export_expr);
    let export = export_target
        .as_ref()
        .and_then(|target| resolve_module_export_in_index(target, &resolve_index));

    Some(SalsaModuleSummary {
        file_id: file_id.id,
        export_target,
        export,
    })
}

fn summarize_export_expr(expr: LuaExpr) -> Option<SalsaExportTargetSummary> {
    match expr {
        LuaExpr::NameExpr(name_expr) => Some(SalsaExportTargetSummary::LocalName(
            name_expr.get_name_text()?.into(),
        )),
        LuaExpr::IndexExpr(index_expr) => summarize_member_export(index_expr),
        LuaExpr::ClosureExpr(closure) => Some(SalsaExportTargetSummary::Closure(u32::from(
            closure.get_position(),
        ))),
        LuaExpr::TableExpr(table_expr) => Some(SalsaExportTargetSummary::Table(u32::from(
            table_expr.get_position(),
        ))),
        LuaExpr::ParenExpr(paren_expr) => summarize_export_expr(paren_expr.get_expr()?),
        _ => None,
    }
}

fn summarize_member_export(
    index_expr: emmylua_parser::LuaIndexExpr,
) -> Option<SalsaExportTargetSummary> {
    let path = extract_member_target_from_index_expr(&index_expr)?;
    Some(SalsaExportTargetSummary::Member(path))
}

fn extract_member_target_from_index_expr(
    index_expr: &emmylua_parser::LuaIndexExpr,
) -> Option<SalsaMemberPathSummary> {
    let (root, segments) = extract_member_path_from_index_expr(index_expr)?;
    let (member_name, owner_segments) = split_member_path(segments)?;
    Some(SalsaMemberPathSummary {
        root,
        owner_segments: owner_segments.into(),
        member_name,
    })
}

fn extract_member_path_from_index_expr(
    index_expr: &emmylua_parser::LuaIndexExpr,
) -> Option<(SalsaMemberPathRootSummary, Vec<SmolStr>)> {
    let (root, mut segments) = extract_member_path_from_expr(&index_expr.get_prefix_expr()?)?;
    segments.push(get_member_name(index_expr)?);
    Some((root, segments))
}

fn extract_member_path_from_expr(
    expr: &LuaExpr,
) -> Option<(SalsaMemberPathRootSummary, Vec<SmolStr>)> {
    match expr {
        LuaExpr::NameExpr(name_expr) => {
            let name = name_expr.get_name_text()?;
            let root = if name == "_G" || name == "_ENV" {
                SalsaMemberPathRootSummary::Env
            } else {
                SalsaMemberPathRootSummary::Name(name.into())
            };
            Some((root, Vec::new()))
        }
        LuaExpr::IndexExpr(index_expr) => extract_member_path_from_index_expr(index_expr),
        LuaExpr::ParenExpr(paren_expr) => extract_member_path_from_expr(&paren_expr.get_expr()?),
        _ => None,
    }
}

fn split_member_path(segments: Vec<SmolStr>) -> Option<(SmolStr, Vec<SmolStr>)> {
    let member_name = segments.last()?.clone();
    let owner_segments = if segments.len() > 1 {
        segments[..segments.len() - 1].to_vec()
    } else {
        Vec::new()
    };
    Some((member_name, owner_segments))
}

fn get_member_name(index_expr: &emmylua_parser::LuaIndexExpr) -> Option<SmolStr> {
    match index_expr.get_index_key()? {
        emmylua_parser::LuaIndexKey::Name(name) => Some(name.get_name_text().into()),
        emmylua_parser::LuaIndexKey::String(string) => Some(string.get_value().into()),
        _ => None,
    }
}
