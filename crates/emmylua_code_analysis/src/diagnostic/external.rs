//! Extension point for third-party (runtime specific) diagnostics.
//!
//! A downstream crate can implement [`ExternalChecker`] and register instances
//! with [`register_external_checker`]. Registered checkers are executed for every
//! analyzed file, after the built-in checkers, and their diagnostics are merged
//! into the result returned by [`crate::LuaDiagnostic::diagnose_file`].
//!
//! Checkers receive read-only access to the database, the semantic model and the
//! document, plus the raw `plugin` section of the configuration
//! (`Emmyrc::plugin`), so they can implement their own enable/disable/severity
//! handling without touching the built-in [`crate::DiagnosticCode`] enum.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use lsp_types::{Diagnostic, DiagnosticSeverity, DiagnosticTag, NumberOrString};
use rowan::TextRange;
use serde_json::Value;

use crate::{FileId, db_index::DbIndex, semantic::SemanticModel, vfs::LuaDocument};

/// Read-only context passed to every external checker.
pub struct ExternalContext<'a> {
    pub file_id: FileId,
    pub db: &'a DbIndex,
    pub semantic_model: &'a SemanticModel<'a>,
    pub document: LuaDocument<'a>,
    /// Raw `plugin` section of the configuration. Runtime-specific settings are
    /// expected to live under a key named after the runtime, e.g.
    /// `plugin.tarantool`.
    pub plugin: &'a HashMap<String, Value>,
}

/// A diagnostic produced by an external checker.
#[derive(Debug, Clone)]
pub struct ExternalDiagnostic {
    /// Stable diagnostic code, e.g. `tarantool/box-cfg-required`.
    pub code: String,
    /// Offsets in the analyzed file.
    pub range: TextRange,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub tags: Option<Vec<DiagnosticTag>>,
    /// LSP diagnostic source, defaults to `Plugin`.
    pub source: Option<String>,
    pub data: Option<Value>,
}

impl ExternalDiagnostic {
    pub fn new(code: impl Into<String>, range: TextRange, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            range,
            severity: DiagnosticSeverity::WARNING,
            message: message.into(),
            tags: None,
            source: None,
            data: None,
        }
    }

    pub fn with_severity(mut self, severity: DiagnosticSeverity) -> Self {
        self.severity = severity;
        self
    }

    pub fn with_tags(mut self, tags: Vec<DiagnosticTag>) -> Self {
        self.tags = Some(tags);
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

/// Implemented by downstream crates to add runtime specific diagnostics.
pub trait ExternalChecker: Send + Sync {
    /// Stable, human readable checker name (used for logging only).
    fn name(&self) -> &'static str;

    fn check(&self, context: &ExternalContext, emit: &mut dyn FnMut(ExternalDiagnostic));
}

static REGISTRY: OnceLock<Mutex<Vec<Box<dyn ExternalChecker>>>> = OnceLock::new();

/// Register a checker to be executed for every analyzed file.
///
/// Should be called once at process start, before any file is diagnosed.
pub fn register_external_checker(checker: Box<dyn ExternalChecker>) {
    let registry = REGISTRY.get_or_init(|| Mutex::new(Vec::new()));
    match registry.lock() {
        Ok(mut checkers) => checkers.push(checker),
        Err(err) => log::error!("failed to register external checker: {}", err),
    }
}

/// Returns true if at least one external checker has been registered.
pub fn has_external_checkers() -> bool {
    REGISTRY
        .get()
        .and_then(|registry| registry.lock().ok().map(|checkers| !checkers.is_empty()))
        .unwrap_or(false)
}

/// Run all registered checkers for a file and return LSP diagnostics.
pub(crate) fn run_external(
    file_id: FileId,
    db: &DbIndex,
    semantic_model: &SemanticModel,
) -> Vec<Diagnostic> {
    let Some(registry) = REGISTRY.get() else {
        return Vec::new();
    };
    let Ok(checkers) = registry.lock() else {
        return Vec::new();
    };
    if checkers.is_empty() {
        return Vec::new();
    }

    if db.get_module_index().is_meta_file(&file_id) {
        return Vec::new();
    }

    let Some(document) = db.get_vfs().get_document(&file_id) else {
        return Vec::new();
    };
    let plugin = &db.get_emmyrc().plugin;

    let context = ExternalContext {
        file_id,
        db,
        semantic_model,
        document,
        plugin,
    };

    let mut result = Vec::new();
    for checker in checkers.iter() {
        let mut emitted = Vec::new();
        checker.check(&context, &mut |diagnostic| emitted.push(diagnostic));
        for diagnostic in emitted {
            match translate_diagnostic(&context, diagnostic) {
                Some(diagnostic) => result.push(diagnostic),
                None => log::warn!(
                    "external checker `{}` produced a diagnostic with an invalid range",
                    checker.name()
                ),
            }
        }
    }

    result
}

fn translate_diagnostic(
    context: &ExternalContext,
    diagnostic: ExternalDiagnostic,
) -> Option<Diagnostic> {
    let start = context.document.get_line_col(diagnostic.range.start())?;
    let end = context.document.get_line_col(diagnostic.range.end())?;
    let range = lsp_types::Range {
        start: lsp_types::Position {
            line: start.0 as u32,
            character: start.1 as u32,
        },
        end: lsp_types::Position {
            line: end.0 as u32,
            character: end.1 as u32,
        },
    };

    Some(Diagnostic {
        range,
        severity: Some(diagnostic.severity),
        code: Some(NumberOrString::String(diagnostic.code)),
        code_description: None,
        source: Some(diagnostic.source.unwrap_or_else(|| "Plugin".to_string())),
        message: diagnostic.message,
        related_information: None,
        tags: diagnostic.tags,
        data: diagnostic.data,
    })
}
