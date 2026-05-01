//! Screen routing: every screen owns its state, render, and input handling.

pub mod projects;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Projects,
    Approvals,
    Rules,
    Audit,
}

impl Screen {
    /// Tab cycle: Projects → Approvals → Rules → Audit → Projects.
    pub fn next(self) -> Self {
        match self {
            Self::Projects => Self::Approvals,
            Self::Approvals => Self::Rules,
            Self::Rules => Self::Audit,
            Self::Audit => Self::Projects,
        }
    }

    /// Reverse Tab cycle.
    pub fn prev(self) -> Self {
        match self {
            Self::Projects => Self::Audit,
            Self::Approvals => Self::Projects,
            Self::Rules => Self::Approvals,
            Self::Audit => Self::Rules,
        }
    }
}
