#[allow(unused)]
mod analyzer;
mod decl;
mod index;
mod member;
mod module;
mod return_flow;
mod summary_builder;
mod test;
mod types;

use emmylua_parser::LuaAstNode;
use hashbrown::HashMap;
use hashbrown::HashSet;
use lsp_types::Uri;
use std::sync::Arc;

pub(crate) use self::summary_builder::analyze_module_return_points;
use crate::{
    DiagnosticActionKind, DiagnosticCode, Emmyrc, FileId, LuaIndex, LuaInferCache, LuaMemberId,
    LuaSemanticDeclId, LuaType, Workspace as LuaWorkspace, WorkspaceId as LuaWorkspaceId,
    db_index::{DbIndex, DiagnosticAction},
    module_query::{
        export::{
            infer_module_export_type, module_export_expr, semantic_id_from_compilation_module,
        },
        identity::find_compilation_module,
    },
    semantic::SemanticModel,
};
pub use decl::{CompilationDeclIndex, CompilationDeclTree};
use index::{CompilationIndexContext, FileBackedIndex};
pub use member::{
    CompilationMemberFeature, CompilationMemberIndex, CompilationMemberInfo, CompilationMemberKind,
    CompilationMemberSource,
};
pub use module::{
    CompilationModuleIndex, CompilationModuleInfo, CompilationModuleNode, CompilationModuleNodeId,
    CompilationModuleVisibility,
};
pub use return_flow::{
    LuaReturnPoint, analyze_func_body_missing_return_flags_with, analyze_func_body_returns_with,
    does_func_body_always_return_or_exit,
};
pub use summary_builder::*;
pub use types::{
    CompilationTypeDecl, CompilationTypeDeclId, CompilationTypeDeclScope, CompilationTypeDeclTree,
    CompilationTypeIndex,
};

#[derive(Debug)]
pub struct LuaCompilation {
    db: DbIndex,
    emmyrc: Arc<Emmyrc>,
    summary: SalsaSummaryHost,
    decls: CompilationDeclIndex,
    modules: CompilationModuleIndex,
    types: CompilationTypeIndex,
    members: CompilationMemberIndex,
}

impl LuaCompilation {
    pub fn new(emmyrc: Arc<Emmyrc>) -> Self {
        let mut compilation = Self {
            db: DbIndex::new(),
            emmyrc: emmyrc.clone(),
            summary: SalsaSummaryHost::new(emmyrc.clone()),
            decls: CompilationDeclIndex::new(),
            modules: CompilationModuleIndex::new(),
            types: CompilationTypeIndex::new(),
            members: CompilationMemberIndex::new(),
        };

        compilation.db.update_config(emmyrc.clone());
        compilation.modules.update_config(emmyrc);
        compilation.sync_summary_workspaces();
        compilation
    }

    fn sync_summary_workspaces(&mut self) {
        let workspaces = self.db.get_module_index().get_workspaces().to_vec();
        self.summary.set_workspaces(workspaces);
        self.modules
            .set_workspaces(self.db.get_module_index().get_workspaces().to_vec());
    }

    // Rebuild summary salsa inputs from the summary host Vfs after index-wide resets.
    fn sync_summary_files(&mut self, file_ids: &[FileId]) {
        for file_id in file_ids {
            if !self.summary.sync_file(*file_id) {
                self.summary.remove_file(*file_id);
                FileBackedIndex::remove_file(&mut self.decls, *file_id);
                FileBackedIndex::remove_file(&mut self.modules, *file_id);
                FileBackedIndex::remove_file(&mut self.types, *file_id);
                FileBackedIndex::remove_file(&mut self.members, *file_id);
                self.db.get_file_dependencies_index_mut().remove(*file_id);
                continue;
            }

            let base_ctx =
                CompilationIndexContext::new(&self.db, &self.summary, self.summary.vfs());
            FileBackedIndex::sync_file(&mut self.decls, &base_ctx, *file_id);
            FileBackedIndex::sync_file(&mut self.modules, &base_ctx, *file_id);

            let type_ctx = base_ctx.with_modules(&self.modules);
            FileBackedIndex::sync_file(&mut self.types, &type_ctx, *file_id);

            let member_ctx = type_ctx.with_types(&self.types);
            FileBackedIndex::sync_file(&mut self.members, &member_ctx, *file_id);

            self.sync_summary_require_dependencies(*file_id);
        }
    }

    fn sync_summary_require_dependencies(&mut self, file_id: FileId) {
        self.db.get_file_dependencies_index_mut().remove(file_id);

        let Some(required_modules) = self.summary.semantic().file().required_modules(file_id)
        else {
            return;
        };

        for module_path in required_modules {
            let Some(module_info) = self.modules.find_module(module_path.as_str()) else {
                continue;
            };

            self.db
                .get_file_dependencies_index_mut()
                .add_required_file(file_id, module_info.file_id);
        }
    }

    fn write_local_file(&mut self, uri: &Uri, text: Option<String>) -> FileId {
        let summary_file_id = self.summary.update_file_by_uri(uri, text.clone());
        let file_id = self.db.get_vfs_mut().set_file_content(uri, text);
        debug_assert_eq!(summary_file_id, file_id);
        file_id
    }

    fn write_remote_file(&mut self, uri: &Uri, text: Option<String>) -> FileId {
        let summary_file_id = self.summary.update_remote_file_by_uri(uri, text.clone());
        let file_id = self.db.get_vfs_mut().set_remote_file_content(uri, text);
        debug_assert_eq!(summary_file_id, file_id);
        file_id
    }

    pub fn get_semantic_model(&'_ self, file_id: FileId) -> Option<SemanticModel<'_>> {
        let cache = LuaInferCache::new(file_id, Default::default());
        let tree = self.db.get_vfs().get_syntax_tree(&file_id)?;
        Some(SemanticModel::new(
            file_id,
            self,
            &self.db,
            &self.summary,
            cache,
            self.emmyrc.clone(),
            tree.get_chunk_node(),
        ))
    }

    pub fn update_index(&mut self, file_ids: Vec<FileId>) {
        self.sync_summary_workspaces();
        self.sync_summary_files(&file_ids);

        for file_id in &file_ids {
            let tree = match self.db.get_vfs().get_syntax_tree(file_id) {
                Some(tree) => tree,
                None => {
                    log::warn!("file_id {:?} not found in vfs", file_id);
                    continue;
                }
            };
            let _ = tree.get_chunk_node();
        }

        // Keep the legacy analyzer pipeline disconnected until compilation owns the
        // remaining semantic/diagnostic consumers.
        self.sync_summary_doc_diagnostics(&file_ids);
    }

    fn sync_summary_doc_diagnostics(&mut self, file_ids: &[FileId]) {
        for file_id in file_ids {
            let Some(properties) = self.summary.doc().tag_properties(*file_id) else {
                continue;
            };

            for property in properties {
                let owner = property.owner.clone();
                let Some(diagnostics) = self
                    .summary
                    .doc()
                    .resolved_tag_diagnostics(*file_id, owner.clone())
                else {
                    continue;
                };

                for diagnostic in diagnostics {
                    self.apply_summary_doc_diagnostic(*file_id, &owner, diagnostic);
                }
            }
        }
    }

    fn apply_summary_doc_diagnostic(
        &mut self,
        file_id: FileId,
        owner: &summary_builder::SalsaDocOwnerSummary,
        diagnostic: crate::SalsaResolvedDocDiagnosticActionSummary,
    ) {
        let diagnostic_index = self.db.get_diagnostic_index_mut();
        let is_ownerless = owner.syntax_offset.is_none();

        match diagnostic.kind {
            summary_builder::SalsaDocDiagnosticActionKindSummary::Enable => {
                if let Some(code) = diagnostic.code {
                    diagnostic_index.add_file_diagnostic_enabled(file_id, code);
                }
            }
            summary_builder::SalsaDocDiagnosticActionKindSummary::Disable if is_ownerless => {
                if let Some(code) = diagnostic.code {
                    diagnostic_index.add_file_diagnostic_disabled(file_id, code);
                } else {
                    diagnostic_index.add_diagnostic_action(
                        file_id,
                        DiagnosticAction::new(diagnostic.range, DiagnosticActionKind::DisableAll),
                    );
                }
            }
            summary_builder::SalsaDocDiagnosticActionKindSummary::Disable
            | summary_builder::SalsaDocDiagnosticActionKindSummary::DisableNextLine
            | summary_builder::SalsaDocDiagnosticActionKindSummary::DisableLine => {
                let kind = diagnostic_action_kind_from_summary(diagnostic.kind, diagnostic.code);
                if let Some(kind) = kind {
                    diagnostic_index.add_diagnostic_action(
                        file_id,
                        DiagnosticAction::new(diagnostic.range, kind),
                    );
                }
            }
        }
    }

    pub fn remove_index(&mut self, file_ids: Vec<FileId>) {
        for file_id in &file_ids {
            self.summary.remove_file(*file_id);
            FileBackedIndex::remove_file(&mut self.decls, *file_id);
            FileBackedIndex::remove_file(&mut self.modules, *file_id);
            FileBackedIndex::remove_file(&mut self.types, *file_id);
            FileBackedIndex::remove_file(&mut self.members, *file_id);
        }
        self.db.remove_index(file_ids);
    }

    pub fn clear_index(&mut self) {
        self.db.clear();
        self.summary.clear();
        FileBackedIndex::clear(&mut self.decls);
        FileBackedIndex::clear(&mut self.modules);
        FileBackedIndex::clear(&mut self.types);
        FileBackedIndex::clear(&mut self.members);
        self.sync_summary_workspaces();
    }

    pub fn get_db(&self) -> &DbIndex {
        &self.db
    }

    pub fn get_db_mut(&mut self) -> &mut DbIndex {
        &mut self.db
    }

    pub fn add_workspace(&mut self, workspace: LuaWorkspace) {
        self.db
            .get_module_index_mut()
            .add_workspace_root_with_import(workspace.root, workspace.import, workspace.id);
        self.sync_summary_workspaces();
    }

    pub fn clear_non_std_workspaces(&mut self) {
        self.db.get_module_index_mut().clear_non_std_workspaces();
        self.sync_summary_workspaces();
    }

    pub fn update_file_by_uri(&mut self, uri: &Uri, text: Option<String>) -> Option<FileId> {
        let is_removed = text.is_none();
        let file_id = self.write_local_file(uri, text);

        self.remove_index(vec![file_id]);
        if !is_removed {
            self.update_index(vec![file_id]);
        }

        Some(file_id)
    }

    pub fn update_remote_file_by_uri(&mut self, uri: &Uri, text: Option<String>) -> FileId {
        let is_removed = text.is_none();
        let file_id = self.write_remote_file(uri, text);

        self.remove_index(vec![file_id]);
        if !is_removed {
            self.update_index(vec![file_id]);
        }

        file_id
    }

    pub fn update_files_by_uri(&mut self, files: Vec<(Uri, Option<String>)>) -> Vec<FileId> {
        let mut removed_files = HashSet::new();
        let mut updated_files = HashSet::new();

        for (uri, text) in files {
            let is_new_text = text.is_some();
            let file_id = self.write_local_file(&uri, text);
            removed_files.insert(file_id);
            if is_new_text {
                updated_files.insert(file_id);
            }
        }

        self.remove_index(removed_files.into_iter().collect());
        let updated_files: Vec<FileId> = updated_files.into_iter().collect();
        self.update_index(updated_files.clone());
        updated_files
    }

    pub(crate) fn update_files_by_uri_sorted(
        &mut self,
        files: Vec<(Uri, Option<String>)>,
    ) -> Vec<FileId> {
        let mut removed_files = HashSet::new();
        let mut updated_files = HashSet::new();

        for (uri, text) in files {
            let is_new_text = text.is_some();
            let file_id = self.write_local_file(&uri, text);
            removed_files.insert(file_id);
            if is_new_text {
                updated_files.insert(file_id);
            }
        }

        self.remove_index(removed_files.into_iter().collect());
        let mut updated_files: Vec<FileId> = updated_files.into_iter().collect();
        updated_files.sort();
        self.update_index(updated_files.clone());
        updated_files
    }

    pub fn remove_file_by_uri(&mut self, uri: &Uri) -> Option<FileId> {
        let summary_file_id = self.summary.remove_file_by_uri(uri);
        let file_id = self.db.get_vfs_mut().remove_file(uri)?;
        debug_assert_eq!(summary_file_id, Some(file_id));
        self.remove_index(vec![file_id]);
        Some(file_id)
    }

    pub fn summary(&self) -> &SalsaSummaryHost {
        &self.summary
    }

    pub fn module_index(&self) -> &CompilationModuleIndex {
        &self.modules
    }

    pub fn find_module_by_file_id(&self, file_id: FileId) -> Option<&CompilationModuleInfo> {
        self.modules.get_module(file_id)
    }

    pub fn module_infos(&self) -> Vec<&CompilationModuleInfo> {
        self.modules.get_module_infos()
    }

    pub fn find_module_node(&self, module_path: &str) -> Option<&CompilationModuleNode> {
        self.modules.find_module_node(module_path)
    }

    pub fn get_module_node(
        &self,
        module_id: &CompilationModuleNodeId,
    ) -> Option<&CompilationModuleNode> {
        self.modules.get_module_node(module_id)
    }

    pub fn module_workspace_id(&self, file_id: FileId) -> Option<LuaWorkspaceId> {
        self.modules.get_workspace_id(file_id)
    }

    pub fn module_is_meta_file(&self, file_id: FileId) -> bool {
        self.modules.is_meta_file(&file_id)
    }

    pub fn module_is_std(&self, file_id: FileId) -> bool {
        self.modules.is_std(&file_id)
    }

    pub fn module_is_main(&self, file_id: FileId) -> bool {
        self.modules.is_main(&file_id)
    }

    pub fn module_is_library(&self, file_id: FileId) -> bool {
        self.modules.is_library(&file_id)
    }

    pub fn std_file_ids(&self) -> Vec<FileId> {
        self.modules.get_std_file_ids()
    }

    pub fn main_workspace_file_ids(&self) -> Vec<FileId> {
        self.modules.get_main_workspace_file_ids()
    }

    pub fn library_file_ids(&self) -> Vec<FileId> {
        self.modules.get_lib_file_ids()
    }

    pub fn next_library_workspace_id(&self) -> u32 {
        self.modules.next_library_workspace_id()
    }

    pub fn decl_index(&self) -> &CompilationDeclIndex {
        &self.decls
    }

    pub fn find_module_by_require_path(&self, module_path: &str) -> Option<&CompilationModuleInfo> {
        find_compilation_module(&self.modules, module_path)
    }

    pub fn find_required_module_export_type(&self, module_path: &str) -> Option<LuaType> {
        let module = self.find_module_by_require_path(module_path)?;
        infer_module_export_type(&self.db, module.file_id)
    }

    pub fn find_required_module_semantic_id(&self, module_path: &str) -> Option<LuaSemanticDeclId> {
        let module = self.find_module_by_require_path(module_path)?;
        semantic_id_from_compilation_module(module).or_else(|| {
            match module_export_expr(&self.db, module.file_id)? {
                emmylua_parser::LuaExpr::IndexExpr(index_expr) => Some(LuaSemanticDeclId::Member(
                    LuaMemberId::new(index_expr.get_syntax_id(), module.file_id),
                )),
                _ => None,
            }
        })
    }

    pub fn type_index(&self) -> &CompilationTypeIndex {
        &self.types
    }

    pub fn member_index(&self) -> &CompilationMemberIndex {
        &self.members
    }

    pub fn get_merged_owner_members(
        &self,
        owner: &CompilationTypeDeclId,
    ) -> HashMap<smol_str::SmolStr, CompilationMemberInfo> {
        self.members.get_merged_owner_members(
            &self.types,
            owner,
            self.emmyrc.strict.meta_override_file_define,
        )
    }

    pub fn get_merged_member(
        &self,
        owner: &CompilationTypeDeclId,
        name: &str,
    ) -> Option<CompilationMemberInfo> {
        self.members.get_merged_member(
            &self.types,
            owner,
            name,
            self.emmyrc.strict.meta_override_file_define,
        )
    }

    pub fn find_type_merged_member(
        &self,
        file_id: FileId,
        type_name: &str,
        workspace_id: Option<LuaWorkspaceId>,
        member_name: &str,
    ) -> Option<CompilationMemberInfo> {
        let type_decl = self
            .types
            .find_type_decl(file_id, type_name, workspace_id)?;
        self.get_merged_member(&type_decl.id, member_name)
    }

    pub fn update_config(&mut self, config: Arc<Emmyrc>) {
        self.emmyrc = config.clone();
        self.db.update_config(config.clone());
        self.summary.update_config(config);
        self.modules.update_config(self.emmyrc.clone());
        self.sync_summary_workspaces();
    }
}

fn diagnostic_action_kind_from_summary(
    kind: summary_builder::SalsaDocDiagnosticActionKindSummary,
    code: Option<DiagnosticCode>,
) -> Option<DiagnosticActionKind> {
    match kind {
        summary_builder::SalsaDocDiagnosticActionKindSummary::Disable
        | summary_builder::SalsaDocDiagnosticActionKindSummary::DisableNextLine
        | summary_builder::SalsaDocDiagnosticActionKindSummary::DisableLine => match code {
            Some(code) => Some(DiagnosticActionKind::Disable(code)),
            None => Some(DiagnosticActionKind::DisableAll),
        },
        summary_builder::SalsaDocDiagnosticActionKindSummary::Enable => {
            code.map(DiagnosticActionKind::Enable)
        }
    }
}
