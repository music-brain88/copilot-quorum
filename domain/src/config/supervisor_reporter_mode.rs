//! Supervisor reporter mode value object

/// Whether the supervisor status reporter (e.g. `HerdrReporterAdapter`) is active.
///
/// This only selects the *policy*; whether a concrete reporting backend is
/// actually reachable (e.g. `HERDR_ENV` present) is decided by the adapter
/// itself at construction time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SupervisorReporterMode {
    /// Enabled only when a supervisor environment is detected (e.g. `HERDR_ENV`).
    #[default]
    Auto,
    /// Always disabled.
    None,
}

impl std::str::FromStr for SupervisorReporterMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(SupervisorReporterMode::Auto),
            "none" => Ok(SupervisorReporterMode::None),
            _ => Err(format!(
                "invalid supervisor reporter mode '{}', valid: auto, none",
                s
            )),
        }
    }
}

impl std::fmt::Display for SupervisorReporterMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SupervisorReporterMode::Auto => write!(f, "auto"),
            SupervisorReporterMode::None => write!(f, "none"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_auto() {
        assert_eq!(
            SupervisorReporterMode::default(),
            SupervisorReporterMode::Auto
        );
    }

    #[test]
    fn test_from_str_roundtrip() {
        assert_eq!(
            "auto".parse::<SupervisorReporterMode>().unwrap(),
            SupervisorReporterMode::Auto
        );
        assert_eq!(
            "none".parse::<SupervisorReporterMode>().unwrap(),
            SupervisorReporterMode::None
        );
        assert!("bogus".parse::<SupervisorReporterMode>().is_err());
    }

    #[test]
    fn test_display_matches_from_str() {
        for mode in [SupervisorReporterMode::Auto, SupervisorReporterMode::None] {
            assert_eq!(
                mode.to_string().parse::<SupervisorReporterMode>().unwrap(),
                mode
            );
        }
    }
}
