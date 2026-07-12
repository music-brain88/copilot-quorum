//! Objection ledger for adversarial Quorum strategies (Debate, etc.)
//!
//! During a `Debate` strategy run, the critic side raises structured
//! rebuttals (`CLAIM` / `EVIDENCE` / `SEVERITY`). This module tracks those
//! rebuttals as [`Objection`]s in an [`ObjectionLedger`] so the moderator's
//! checkpoint rulings (conceded / refuted / unresolved) can be recorded and
//! queried without re-parsing raw transcript text.
//!
//! This is pure, `async`-independent domain logic (no I/O, no LLM calls),
//! following the precedent set by `quorum::vote` (#212).
//!
//! # Example
//!
//! ```
//! use quorum_domain::quorum::{ObjectionLedger, ObjectionSeverity, ObjectionStatus};
//!
//! let mut ledger = ObjectionLedger::new();
//! let id = ledger.add(1, "The plan ignores concurrent writes", "See race in step 3", ObjectionSeverity::Major);
//! assert_eq!(id, "R1-1");
//!
//! ledger.apply_ruling(&id, true, "Critic's evidence holds; proposer failed to address it");
//! assert_eq!(ledger.open_objections().len(), 0);
//! ```

use serde::{Deserialize, Serialize};

/// How serious an objection is, as classified by the critic's `SEVERITY:` line.
///
/// Maps directly onto the `CRITICAL` / `MAJOR` / `MINOR` vocabulary used in
/// the Debate prompt template (`domain/src/prompt/debate.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectionSeverity {
    /// Breaks the position entirely.
    Critical,
    /// A real gap that needs addressing.
    Major,
    /// A nitpick worth noting.
    Minor,
}

/// Resolution state of an [`Objection`] as determined by the moderator's ruling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectionStatus {
    /// The proposer conceded the point (ruling: valid objection, accepted).
    Conceded,
    /// The moderator ruled the objection invalid / adequately rebutted.
    Refuted,
    /// No ruling has been applied yet.
    Unresolved,
}

/// A single structured rebuttal raised during a Debate round.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Objection {
    /// Stable identifier, e.g. `"R1-1"` (round 1, objection 1).
    pub id: String,
    /// The claim being attacked (the `CLAIM:` line).
    pub claim: String,
    /// The counter-evidence offered (the `EVIDENCE:` line).
    pub evidence: String,
    /// Severity as classified by the critic.
    pub severity: ObjectionSeverity,
    /// Current resolution status.
    pub status: ObjectionStatus,
    /// Moderator's reasoning for the ruling, if one has been applied.
    pub ruling_reason: Option<String>,
}

/// Ledger tracking all objections raised across a Debate run.
///
/// IDs are minted per round as `R{round}-{n}`, where `n` is a 1-based
/// counter local to that round.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ObjectionLedger {
    objections: Vec<Objection>,
}

impl ObjectionLedger {
    /// Create an empty ledger.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a new objection raised in the given round.
    ///
    /// Returns the newly minted objection ID (e.g. `"R2-3"`).
    pub fn add(
        &mut self,
        round: usize,
        claim: impl Into<String>,
        evidence: impl Into<String>,
        severity: ObjectionSeverity,
    ) -> String {
        let n = self
            .objections
            .iter()
            .filter(|o| o.id.starts_with(&format!("R{round}-")))
            .count()
            + 1;
        let id = format!("R{round}-{n}");
        self.objections.push(Objection {
            id: id.clone(),
            claim: claim.into(),
            evidence: evidence.into(),
            severity,
            status: ObjectionStatus::Unresolved,
            ruling_reason: None,
        });
        id
    }

    /// Apply the moderator's ruling to an objection by ID.
    ///
    /// `ruling: true` means the objection was conceded (upheld);
    /// `ruling: false` means it was refuted (rejected).
    ///
    /// Only applies when the objection is still `Unresolved` — a later
    /// round's moderator re-ruling on the same `REBUTTAL_ID` does not
    /// silently overwrite an already-`Conceded`/`Refuted` verdict from an
    /// earlier round. Returns `true` if the ruling was applied, `false` if
    /// the ID was not found or the objection was already resolved (the
    /// caller may want to log a warning on `false`).
    pub fn apply_ruling(&mut self, id: &str, ruling: bool, reason: impl Into<String>) -> bool {
        let Some(objection) = self.objections.iter_mut().find(|o| o.id == id) else {
            return false;
        };
        if objection.status != ObjectionStatus::Unresolved {
            return false;
        }
        objection.status = if ruling {
            ObjectionStatus::Conceded
        } else {
            ObjectionStatus::Refuted
        };
        objection.ruling_reason = Some(reason.into());
        true
    }

    /// All objections in the ledger, in insertion order.
    pub fn all(&self) -> &[Objection] {
        &self.objections
    }

    /// Unresolved objections whose severity is `Critical` or `Major`.
    ///
    /// Used to decide whether a Debate round can settle early: if none
    /// remain, the moderator's checkpoint can proceed to synthesis.
    pub fn unresolved_critical_or_major(&self) -> Vec<&Objection> {
        self.objections
            .iter()
            .filter(|o| {
                o.status == ObjectionStatus::Unresolved
                    && matches!(
                        o.severity,
                        ObjectionSeverity::Critical | ObjectionSeverity::Major
                    )
            })
            .collect()
    }

    /// All objections still awaiting a ruling, regardless of severity.
    ///
    /// Intended for building the moderator checkpoint prompt (list of
    /// still-open points that need a verdict).
    pub fn open_objections(&self) -> Vec<&Objection> {
        self.objections
            .iter()
            .filter(|o| o.status == ObjectionStatus::Unresolved)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_mints_sequential_ids_per_round() {
        let mut ledger = ObjectionLedger::new();
        let id1 = ledger.add(1, "claim a", "evidence a", ObjectionSeverity::Major);
        let id2 = ledger.add(1, "claim b", "evidence b", ObjectionSeverity::Minor);
        let id3 = ledger.add(2, "claim c", "evidence c", ObjectionSeverity::Critical);

        assert_eq!(id1, "R1-1");
        assert_eq!(id2, "R1-2");
        // Round 2 counter starts fresh from 1.
        assert_eq!(id3, "R2-1");
    }

    #[test]
    fn new_objection_starts_unresolved() {
        let mut ledger = ObjectionLedger::new();
        let id = ledger.add(1, "claim", "evidence", ObjectionSeverity::Critical);
        let objection = ledger.all().iter().find(|o| o.id == id).unwrap();

        assert_eq!(objection.status, ObjectionStatus::Unresolved);
        assert!(objection.ruling_reason.is_none());
    }

    #[test]
    fn apply_ruling_conceded() {
        let mut ledger = ObjectionLedger::new();
        let id = ledger.add(1, "claim", "evidence", ObjectionSeverity::Major);
        assert!(ledger.apply_ruling(&id, true, "critic's evidence is solid"));

        let objection = ledger.all().iter().find(|o| o.id == id).unwrap();
        assert_eq!(objection.status, ObjectionStatus::Conceded);
        assert_eq!(
            objection.ruling_reason.as_deref(),
            Some("critic's evidence is solid")
        );
    }

    #[test]
    fn apply_ruling_refuted() {
        let mut ledger = ObjectionLedger::new();
        let id = ledger.add(1, "claim", "evidence", ObjectionSeverity::Minor);
        assert!(ledger.apply_ruling(&id, false, "already addressed in round 1"));

        let objection = ledger.all().iter().find(|o| o.id == id).unwrap();
        assert_eq!(objection.status, ObjectionStatus::Refuted);
    }

    #[test]
    fn apply_ruling_unknown_id_is_noop() {
        let mut ledger = ObjectionLedger::new();
        ledger.add(1, "claim", "evidence", ObjectionSeverity::Major);
        assert!(!ledger.apply_ruling("R99-1", true, "does not exist"));

        // Original objection remains unresolved; nothing panics.
        assert_eq!(ledger.open_objections().len(), 1);
    }

    #[test]
    fn apply_ruling_already_resolved_is_ignored() {
        // A later round's moderator re-mentioning the same REBUTTAL_ID must
        // not silently overwrite an already-decided ruling.
        let mut ledger = ObjectionLedger::new();
        let id = ledger.add(1, "claim", "evidence", ObjectionSeverity::Major);
        assert!(ledger.apply_ruling(&id, false, "refuted in round 1"));

        // A conflicting re-ruling on the same ID is rejected...
        assert!(!ledger.apply_ruling(&id, true, "conceded in round 2 — should not stick"));

        // ...and the original ruling is untouched.
        let objection = ledger.all().iter().find(|o| o.id == id).unwrap();
        assert_eq!(objection.status, ObjectionStatus::Refuted);
        assert_eq!(
            objection.ruling_reason.as_deref(),
            Some("refuted in round 1")
        );
    }

    #[test]
    fn unresolved_critical_or_major_excludes_minor_and_resolved() {
        let mut ledger = ObjectionLedger::new();
        let critical_id = ledger.add(1, "c1", "e1", ObjectionSeverity::Critical);
        let major_id = ledger.add(1, "c2", "e2", ObjectionSeverity::Major);
        let minor_id = ledger.add(1, "c3", "e3", ObjectionSeverity::Minor);

        // Minor is excluded regardless of status.
        let unresolved = ledger.unresolved_critical_or_major();
        assert_eq!(unresolved.len(), 2);
        assert!(unresolved.iter().any(|o| o.id == critical_id));
        assert!(unresolved.iter().any(|o| o.id == major_id));
        assert!(!unresolved.iter().any(|o| o.id == minor_id));

        // Resolving the critical one shrinks the set.
        ledger.apply_ruling(&critical_id, false, "rebutted");
        assert_eq!(ledger.unresolved_critical_or_major().len(), 1);
    }

    #[test]
    fn open_objections_only_returns_unresolved() {
        let mut ledger = ObjectionLedger::new();
        let id1 = ledger.add(1, "c1", "e1", ObjectionSeverity::Critical);
        let id2 = ledger.add(1, "c2", "e2", ObjectionSeverity::Minor);

        assert_eq!(ledger.open_objections().len(), 2);

        ledger.apply_ruling(&id1, true, "conceded");
        let open = ledger.open_objections();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].id, id2);
    }

    #[test]
    fn severity_serde_snake_case() {
        assert_eq!(
            serde_json::to_value(ObjectionSeverity::Critical).unwrap(),
            "critical"
        );
        assert_eq!(
            serde_json::to_value(ObjectionSeverity::Major).unwrap(),
            "major"
        );
        assert_eq!(
            serde_json::to_value(ObjectionSeverity::Minor).unwrap(),
            "minor"
        );
    }

    #[test]
    fn status_serde_snake_case() {
        assert_eq!(
            serde_json::to_value(ObjectionStatus::Conceded).unwrap(),
            "conceded"
        );
        assert_eq!(
            serde_json::to_value(ObjectionStatus::Refuted).unwrap(),
            "refuted"
        );
        assert_eq!(
            serde_json::to_value(ObjectionStatus::Unresolved).unwrap(),
            "unresolved"
        );
    }

    #[test]
    fn empty_ledger_has_no_open_or_unresolved_objections() {
        let ledger = ObjectionLedger::new();
        assert!(ledger.open_objections().is_empty());
        assert!(ledger.unresolved_critical_or_major().is_empty());
        assert!(ledger.all().is_empty());
    }
}
