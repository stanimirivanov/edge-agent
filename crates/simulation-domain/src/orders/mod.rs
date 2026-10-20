//! Event-applied dry-run orders, not funded admission or a broker abstraction.
//! P08 owns authoritative account admission/persistence and wire contracts;
//! P09 qualifies fill models. Domain facts remain uncommitted until that boundary.
//!
//! ```
//! use edgeagent_simulation_domain::orders::{DryRunMode, OrderQuantity};
//! assert!(DryRunMode::parse(None).is_err());
//! let mode = DryRunMode::parse(Some("dry_run"))?;
//! let shares = OrderQuantity::shares(10)?;
//! assert_eq!(shares.parts().1, 10);
//! # Ok::<(), edgeagent_simulation_domain::orders::OrderError>(())
//! ```
//!
//! ```compile_fail,E0423
//! use edgeagent_simulation_domain::orders::DryRunMode;
//! let capability = DryRunMode(()); // no unchecked construction
//! ```

mod definition;
mod events;
mod lifecycle;
mod simulation;
mod values;

pub use definition::{OrderDefinition, OrderInstrument, OrderScope};
pub use events::{FillFact, OrderEvent, OrderEventKind, OrderRejection};
pub use lifecycle::{ApplyOutcome, Order, OrderStatus};
pub use simulation::{ProposedFill, SimulationExecution, SimulationRequest, SimulationStep};
pub use values::{
    DryRunMode, OrderError, OrderEventId, OrderId, OrderKind, OrderPrice, OrderQuantity, OrderSide,
    QuantityUnit,
};
