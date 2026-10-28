use super::*;
#[test]
fn native_companion_binds_exact_records_owner_inputs_and_legacy_recovery() -> Result {
    let mut segment = publication()?.segment().data().clone();
    segment.records.first_mut().ok_or("record")?.observation.id = EvidenceId::new(&format!(
        "{:032x}",
        (7u128 << 64) | 9_007_199_254_740_993u128
    ))?;
    let p = publication_with(StreamSegment::new(segment)?)?;
    let c = fixture::companion(p.segment())?;
    c.bind(p.segment())?;
    assert_eq!(StreamCompanion::decode(c.bytes(), &c.object().sha256)?, c);
    let p = p.with_companion(c.clone())?;
    let bytes = p.bytes()?;
    let recovered = StreamPublication::decode(&bytes)?;
    assert_eq!(recovered.bytes()?, bytes);
    assert_eq!(recovered.companion(), Some(&c));
    assert_eq!(
        StreamPublication::objects(&p.descriptor()?)?,
        (p.segment().object(), Some(c.object()))
    );
    let legacy = publication()?;
    assert!(
        StreamPublication::decode(&legacy.bytes()?)?
            .companion()
            .is_none()
    );
    assert_eq!(
        StreamPublication::objects(&legacy.descriptor()?)?,
        (legacy.segment().object(), None)
    );
    assert!(!format!("{c:?}").contains("9007199254740993"));
    let native = c.data().records.first().ok_or("record")?;
    assert!(
        matches!(&native.value,StreamNativeValue::IexTrade{id,conditions,..} if id.as_str()=="9007199254740993" && conditions==&["Z","@"])
    );
    let mut wrong = c.data().clone();
    wrong.records.clear();
    assert!(StreamCompanion::new(wrong)?.bind(p.segment()).is_err());
    let mut foreign = p.segment().data().clone();
    foreign.records.first_mut().ok_or("record")?.observation.id = EvidenceId::new(&format!(
        "{:032x}",
        (8u128 << 64) | 9_007_199_254_740_993u128
    ))?;
    let foreign = StreamSegment::new(foreign)?;
    let mut foreign_native = c.data().clone();
    foreign_native.segment = foreign.object();
    assert!(
        publication_with(foreign)?
            .with_companion(StreamCompanion::new(foreign_native)?)
            .is_err()
    );
    let mut foreign = p.segment().data().clone();
    foreign.records.first_mut().ok_or("record")?.observation.id = EvidenceId::new(&format!(
        "{:032x}",
        (8u128 << 64) | 9_007_199_254_740_993u128
    ))?;
    let foreign = StreamSegment::new(foreign)?;
    let mut foreign_native = c.data().clone();
    foreign_native.segment = foreign.object();
    assert!(
        publication_with(foreign)?
            .with_companion(StreamCompanion::new(foreign_native)?)
            .is_err()
    );
    let mut wrong = c.data().clone();
    if let StreamNativeValue::IexTrade { id, .. } =
        &mut wrong.records.first_mut().ok_or("record")?.value
    {
        *id = EvidenceRevision::new("1")?;
    }
    assert!(StreamCompanion::new(wrong)?.bind(p.segment()).is_err());
    let mut wrong = c.data().clone();
    wrong.segment.delete_by.seconds += 1;
    assert!(StreamCompanion::new(wrong)?.bind(p.segment()).is_err());
    let mut wrong = c.data().clone();
    wrong.limits.bytes = 1;
    assert!(StreamCompanion::new(wrong).is_err());
    let mut wrong = c.data().clone();
    wrong.records.first_mut().ok_or("record")?.item += 1;
    assert!(StreamCompanion::new(wrong)?.bind(p.segment()).is_err());
    let mut wrong = c.data().clone();
    wrong.owner_inputs = b"not approved inputs".to_vec();
    assert!(
        publication()?
            .with_companion(StreamCompanion::new(wrong)?)
            .is_err()
    );
    let mut bytes = c.bytes().to_vec();
    bytes.push(b' ');
    assert!(StreamCompanion::decode(&bytes, &DailyEvidenceArtifact::hash(&bytes)).is_err());
    assert!(
        StreamCompanion::decode(&vec![0; MAX_STREAM_SEGMENT_BYTES + 1], &c.object().sha256)
            .is_err()
    );
    Ok(())
}
