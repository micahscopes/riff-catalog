//! riff-catalog-tool: the catalog's operations over declared units and worlds,
//! as a library and as the recipe-free `riffcat-core` executable.
//!
//! An application (a tool for one kind of artifact) opens a [`Corpus`],
//! declares its recipes to it, admits the units its producers make
//! ([`admission`]), and adds the generic commands ([`commands`]) to its own.
//! A producer in another language writes units as JSON lines and uses
//! `riffcat-core` directly.

pub mod admission;
pub mod claims_cmd;
pub mod commands;
pub mod corpus;
pub mod declarations;
pub mod facet;
pub mod queries;
pub mod table;
pub mod view_cmd;

pub use admission::{admit_files, admit_unit, admit_units, emit_unit};
pub use commands::CoreCommand;
pub use corpus::Corpus;
pub use declarations::Declarations;
