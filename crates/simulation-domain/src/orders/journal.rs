//! Bounded retained facts, not a serialization of unchecked projection fields.
use super::*;
use crate::ArtifactDigest;
use edgeagent_market_data_domain::{
    OptionPoints, PreciseInstant, ReferenceId, ReferenceRevision, SemanticReference, UsdPrice,
};

impl Order {
    /// Encode the complete retained journal with version, catalog binding and
    /// corruption checksum. This is private persistence, not a public wire DTO.
    /// The binding MUST identify the verified immutable original submission.
    #[must_use]
    pub fn journal_bytes(&self, binding: ArtifactDigest) -> Vec<u8> {
        let mut bytes = b"EAOJ1".to_vec();
        bytes.extend(binding.bytes());
        bytes.extend((self.events().len() as u64).to_be_bytes());
        for event in self.events() {
            let (id, _, revision) = event.position();
            bytes.extend(id.bytes());
            bytes.extend(revision.to_be_bytes());
            let (at, kind) = event.effect();
            let (s, n) = at.unix_parts();
            bytes.extend(s.to_be_bytes());
            bytes.extend(n.to_be_bytes());
            let tag = match kind {
                OrderEventKind::Accepted => 0,
                OrderEventKind::Rejected(OrderRejection::Ineligible) => 1,
                OrderEventKind::Rejected(OrderRejection::Risk) => 2,
                OrderEventKind::Rejected(OrderRejection::Funding) => 3,
                OrderEventKind::Rejected(OrderRejection::Evidence) => 4,
                OrderEventKind::CancelRequested => 5,
                OrderEventKind::CancelRejected => 6,
                OrderEventKind::Cancelled => 7,
                OrderEventKind::Expired => 8,
                OrderEventKind::Filled(_) => 9,
                OrderEventKind::NoFill { .. } => 10,
            };
            bytes.push(tag);
            match kind {
                OrderEventKind::Filled(fill) => {
                    let (proposal, model, evidence) = fill.evidence();
                    let (quantity, price) = proposal.values();
                    bytes.push(match quantity.parts().0 {
                        QuantityUnit::Shares => 0,
                        QuantityUnit::Contracts => 1,
                    });
                    bytes.extend(quantity.parts().1.to_be_bytes());
                    let (tag, parts) = match price {
                        OrderPrice::PerShare(p) => (0, p.decimal_parts()),
                        OrderPrice::Premium(p) => (1, p.decimal_parts()),
                    };
                    bytes.push(tag);
                    bytes.extend(parts.0.to_be_bytes());
                    bytes.extend(parts.1.to_be_bytes());
                    provenance(model, evidence, &mut bytes);
                }
                OrderEventKind::NoFill { model, evidence } => {
                    provenance(model, evidence, &mut bytes)
                }
                _ => {}
            }
        }
        bytes.extend(ArtifactDigest::of(&bytes).bytes());
        bytes
    }
    /// Restore by replaying every checked fact, never trusting stored status,
    /// quantities or revisions. Authenticate the catalog binding before calling.
    ///
    /// # Errors
    /// Returns `Evidence` for malformed/binding/checksum/size failures or the
    /// original lifecycle error. Rejects before allocation above 1 MiB/4096 facts.
    pub fn restore_journal(
        definition: OrderDefinition,
        capacity: usize,
        binding: ArtifactDigest,
        bytes: &[u8],
    ) -> Result<Self, OrderError> {
        if bytes.len() > 1_048_576 {
            return Err(OrderError::Capacity);
        }
        let (body, checksum) = bytes
            .split_at_checked(bytes.len().checked_sub(32).ok_or(OrderError::Evidence)?)
            .ok_or(OrderError::Evidence)?;
        if ArtifactDigest::of(body).bytes().as_slice() != checksum {
            return Err(OrderError::Evidence);
        }
        let mut body = body;
        if take::<5>(&mut body)? != *b"EAOJ1" || take::<32>(&mut body)? != binding.bytes() {
            return Err(OrderError::Evidence);
        }
        let count = u64::from_be_bytes(take(&mut body)?);
        if count > 4096 || count > capacity as u64 {
            return Err(OrderError::Capacity);
        }
        let mut order = Self::new(definition, capacity)?;
        for _ in 0..count {
            let id = OrderEventId(take(&mut body)?);
            let revision = u64::from_be_bytes(take(&mut body)?);
            let seconds = i64::from_be_bytes(take(&mut body)?);
            let nanos = u32::try_from(u64::from_be_bytes(take(&mut body)?))
                .map_err(|_| OrderError::Time)?;
            let at =
                PreciseInstant::from_unix_parts(seconds, nanos).map_err(|_| OrderError::Time)?;
            let kind = match take::<1>(&mut body)?[0] {
                0 => OrderEventKind::Accepted,
                1 => OrderEventKind::Rejected(OrderRejection::Ineligible),
                2 => OrderEventKind::Rejected(OrderRejection::Risk),
                3 => OrderEventKind::Rejected(OrderRejection::Funding),
                4 => OrderEventKind::Rejected(OrderRejection::Evidence),
                5 => OrderEventKind::CancelRequested,
                6 => OrderEventKind::CancelRejected,
                7 => OrderEventKind::Cancelled,
                8 => OrderEventKind::Expired,
                9 => {
                    let unit = match take::<1>(&mut body)?[0] {
                        0 => QuantityUnit::Shares,
                        1 => QuantityUnit::Contracts,
                        _ => return Err(OrderError::Unit),
                    };
                    let quantity =
                        OrderQuantity::with_unit(unit, u64::from_be_bytes(take(&mut body)?))?;
                    let tag = take::<1>(&mut body)?[0];
                    let coefficient = i128::from_be_bytes(take(&mut body)?);
                    let scale = u32::from_be_bytes(take(&mut body)?);
                    let price = match tag {
                        0 => OrderPrice::PerShare(
                            UsdPrice::new(coefficient, scale).map_err(|_| OrderError::Price)?,
                        ),
                        1 => OrderPrice::Premium(
                            OptionPoints::new(coefficient, scale).map_err(|_| OrderError::Price)?,
                        ),
                        _ => return Err(OrderError::Unit),
                    };
                    let (model, evidence) = read_provenance(&mut body)?;
                    OrderEventKind::Filled(FillFact::new(
                        ProposedFill::new(quantity, price),
                        model,
                        evidence,
                    ))
                }
                10 => {
                    let (model, evidence) = read_provenance(&mut body)?;
                    OrderEventKind::NoFill { model, evidence }
                }
                _ => return Err(OrderError::Evidence),
            };
            order.apply(OrderEvent::new(
                id,
                order.definition().scope().identity().3,
                revision,
                at,
                kind,
            )?)?;
        }
        if !body.is_empty() {
            return Err(OrderError::Evidence);
        }
        Ok(order)
    }
}
fn provenance(
    model: (SemanticReference, ArtifactDigest),
    evidence: ArtifactDigest,
    out: &mut Vec<u8>,
) {
    let (id, revision) = model.0.version();
    out.extend(id.get().to_be_bytes());
    out.extend(revision.get().to_be_bytes());
    out.extend(model.1.bytes());
    out.extend(evidence.bytes());
}
fn read_provenance(
    bytes: &mut &[u8],
) -> Result<((SemanticReference, ArtifactDigest), ArtifactDigest), OrderError> {
    let id =
        ReferenceId::new(u128::from_be_bytes(take(bytes)?)).map_err(|_| OrderError::Evidence)?;
    let revision = ReferenceRevision::new(u64::from_be_bytes(take(bytes)?))
        .map_err(|_| OrderError::Evidence)?;
    Ok((
        (
            SemanticReference::new(id, revision),
            ArtifactDigest::from_bytes(take(bytes)?),
        ),
        ArtifactDigest::from_bytes(take(bytes)?),
    ))
}
fn take<const N: usize>(bytes: &mut &[u8]) -> Result<[u8; N], OrderError> {
    let (head, tail) = bytes.split_at_checked(N).ok_or(OrderError::Evidence)?;
    *bytes = tail;
    head.try_into().map_err(|_| OrderError::Evidence)
}
