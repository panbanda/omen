//! The gate that turns a threshold violation into an exit status.

use crate::cli::GateMode;

/// Resolve the gate mode of a gated command. The `--gate` flag overrides the
/// `[gate]` value in the configuration.
pub fn resolve_gate_mode(flag: Option<GateMode>, configured: GateMode) -> GateMode {
    flag.unwrap_or(configured)
}

/// Shared gate verdict dispatch used by `complexity`, `score`,
/// `mutation`, and `stubs`. Callers detect the threshold violation themselves
/// (each analyzer's report shape differs) and hand the resulting
/// `Error::ThresholdViolation` to this function only when a violation
/// occurred; this function decides what to do with it:
///
/// - `off`   -- report only, never called with a violation (callers should
///   skip the gate check entirely when the resolved mode is `Off`).
/// - `warn`  -- print a one-line summary to stderr, exit 0 (`Ok`).
/// - `error` -- return the violation, which `main` maps to exit code 2.
pub fn gate_dispatch(gate: GateMode, violation: crate::core::Error) -> crate::core::Result<()> {
    match gate {
        GateMode::Off => Ok(()),
        GateMode::Warn => {
            eprintln!("\nGate mode 'warn': not failing the build.");
            Ok(())
        }
        GateMode::Error => Err(violation),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_gate_mode_uses_the_configuration_without_a_flag() {
        assert_eq!(resolve_gate_mode(None, GateMode::Off), GateMode::Off);
        assert_eq!(resolve_gate_mode(None, GateMode::Error), GateMode::Error);
    }

    #[test]
    fn resolve_gate_mode_flag_overrides_the_configuration() {
        assert_eq!(
            resolve_gate_mode(Some(GateMode::Off), GateMode::Error),
            GateMode::Off
        );
        assert_eq!(
            resolve_gate_mode(Some(GateMode::Warn), GateMode::Off),
            GateMode::Warn
        );
        assert_eq!(
            resolve_gate_mode(Some(GateMode::Error), GateMode::Off),
            GateMode::Error
        );
    }

    #[test]
    fn gate_dispatch_off_never_fails() {
        let violation = crate::core::Error::threshold_violation("violated", 0.0);
        assert!(gate_dispatch(GateMode::Off, violation).is_ok());
    }

    #[test]
    fn gate_dispatch_warn_returns_ok() {
        let violation = crate::core::Error::threshold_violation("violated", 0.0);
        assert!(gate_dispatch(GateMode::Warn, violation).is_ok());
    }

    #[test]
    fn gate_dispatch_error_returns_the_violation() {
        let violation = crate::core::Error::threshold_violation("violated", 42.0);
        let result = gate_dispatch(GateMode::Error, violation);
        assert!(matches!(
            result,
            Err(crate::core::Error::ThresholdViolation { score, .. }) if score == 42.0
        ));
    }
}
