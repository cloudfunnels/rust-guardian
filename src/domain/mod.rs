//! Domain layer for Rust Guardian
//! 
//! CDD Principle: Domain Model - Pure business logic for code quality enforcement
//! - Contains all core entities, value objects, and domain services
//! - Independent of infrastructure concerns like databases, file systems, or external APIs
//! - Expresses the ubiquitous language of code quality and violation detection

pub mod violations;

// Re-export main domain types for convenience
pub use violations::*;