//! Ejecutor: recorre la cadena lineal de bloques, resuelve referencias `$.`,
//! valida schemas, invoca el sandbox y persiste el trace. Desacoplado del
//! transporte HTTP.
//!
//! Piezas:
//! - [`invoker`]: contrato ejecutor ↔ motor de bloques (`BlockInvoker`) y el
//!   invoker de producción sobre rquickjs ([`SandboxInvoker`]).
//! - [`executor`]: la máquina de cadena lineal con fail-fast.
//! - [`store`]: persistencia atómica de `executions` + `execution_steps`.

pub mod executor;
pub mod invoker;
pub mod store;

pub use executor::{ChainRun, ExecutionOutcome, ExecutionStep, Executor};
pub use invoker::{BlockInvoker, InvokeRequest, SandboxInvoker};
