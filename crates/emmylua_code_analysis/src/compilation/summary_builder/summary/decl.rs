use smol_str::SmolStr;

use super::SalsaSyntaxIdSummary;

use crate::SalsaMemberSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, salsa::Update)]
pub struct SalsaDeclId(pub u32);

impl SalsaDeclId {
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl From<u32> for SalsaDeclId {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<SalsaDeclId> for u32 {
    fn from(value: SalsaDeclId) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct SalsaDeclTreeSummary {
    pub file_id: u32,
    pub decls: Vec<SalsaDeclSummary>,
    pub scopes: Vec<SalsaScopeSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct SalsaDeclSummary {
    pub id: SalsaDeclId,
    pub name: SmolStr,
    pub kind: SalsaDeclKindSummary,
    pub syntax_id: Option<SalsaSyntaxIdSummary>,
    pub scope_id: u32,
    pub start_offset: u32,
    pub end_offset: u32,
    pub value_expr_offset: Option<u32>,
    pub value_expr_syntax_id: Option<SalsaSyntaxIdSummary>,
    pub value_result_index: usize,
    pub source_call_syntax_id: Option<SalsaSyntaxIdSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub enum SalsaDeclKindSummary {
    Local {
        attrib: Option<SalsaLocalAttributeSummary>,
    },
    Param {
        idx: usize,
        signature_offset: u32,
    },
    ImplicitSelf,
    Global,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub enum SalsaLocalAttributeSummary {
    Const,
    Close,
    IterConst,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct SalsaScopeSummary {
    pub id: u32,
    pub parent: Option<u32>,
    pub kind: SalsaScopeKindSummary,
    pub start_offset: u32,
    pub end_offset: u32,
    pub children: Vec<SalsaScopeChildSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub enum SalsaScopeKindSummary {
    Normal,
    Repeat,
    LocalOrAssignStat,
    ForRange,
    FuncStat,
    MethodStat,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub enum SalsaScopeChildSummary {
    Scope(u32),
    Decl(SalsaDeclId),
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct SalsaGlobalSummary {
    pub entries: Vec<SalsaGlobalEntrySummary>,
    pub variables: Vec<SalsaGlobalVariableSummary>,
    pub functions: Vec<SalsaGlobalFunctionSummary>,
    pub members: Vec<SalsaMemberSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct SalsaGlobalEntrySummary {
    pub name: SmolStr,
    pub decl_ids: Vec<SalsaDeclId>,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct SalsaGlobalVariableSummary {
    pub name: SmolStr,
    pub decl_id: SalsaDeclId,
    pub value_expr_offset: Option<u32>,
    pub value_expr_syntax_id: Option<SalsaSyntaxIdSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct SalsaGlobalFunctionSummary {
    pub name: SmolStr,
    pub decl_id: Option<SalsaDeclId>,
    pub signature_offset: u32,
    pub is_method: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, salsa::Update)]
pub enum SalsaGlobalRootSummary {
    Env,
    Name(SmolStr),
}
