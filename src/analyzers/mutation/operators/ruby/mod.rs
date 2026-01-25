//! Ruby-specific mutation operators.
//!
//! This module provides mutation operators specific to Ruby's idioms:
//! - `nil` -> `false`, `0`, `""` (nil replacement)
//! - `:symbol` -> `"symbol"` (symbol to string)
//! - `x.nil?` -> `true` (nil check mutation)
//! - `&.` -> `.` (safe navigation removal)
//! - Block emptying

mod nil;
mod symbol;

pub use nil::RubyNilOperator;
pub use symbol::RubySymbolOperator;

use super::super::operator::OperatorRegistry;

/// Register all Ruby-specific operators.
pub fn register_ruby_operators(registry: &mut OperatorRegistry) {
    registry.register(Box::new(RubyNilOperator));
    registry.register(Box::new(RubySymbolOperator));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_ruby_operators() {
        let mut registry = OperatorRegistry::new();
        register_ruby_operators(&mut registry);

        assert_eq!(registry.operators().len(), 2);

        let names: Vec<_> = registry.operators().iter().map(|op| op.name()).collect();
        assert!(names.contains(&"RNR"));
        assert!(names.contains(&"RSM"));
    }
}
