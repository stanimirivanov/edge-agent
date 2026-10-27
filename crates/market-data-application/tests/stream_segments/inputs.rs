use super::*;
use std::num::NonZeroU64;
fn claim(namespace: u64, scope: &[u8]) -> Result<StreamNamespaceClaim> {
    Ok(StreamNamespaceClaim::new(
        NonZeroU64::new(namespace).ok_or("namespace")?,
        scope,
    )?)
}
#[test]
fn stream_owner_inputs_bind_complete_bytes_and_reject_ambiguous_or_stale_records() -> Result {
    let f = Fixture::new(AT, 1)?;
    let mut data = f.data(1)?;
    let profile = b"invented complete private profile";
    data.configuration = DailyEvidenceArtifact::hash(profile);
    let session = f.seal(data)?;
    let claims = [
        claim(u64::MAX, br#"["scope",1]"#)?,
        claim(7, br#"["scope",2]"#)?,
    ];
    let inputs = StreamOwnerInputs::new(&session, profile, &claims, at(AT + 1000)?)?;
    assert_eq!(StreamOwnerInputs::decode(inputs.bytes())?, inputs);
    assert_eq!(inputs.profile(), profile);
    assert_eq!(inputs.claims().last().ok_or("claim")?.namespace(), u64::MAX);
    assert_eq!(
        inputs,
        StreamOwnerInputs::new(
            &session,
            profile,
            &[claims[1].clone(), claims[0].clone()],
            at(AT + 1000)?
        )?
    );
    inputs.recheck(
        &session,
        f.catalogs(),
        at(AT + 1)?,
        &Cancellation::default(),
    )?;
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(
        inputs
            .recheck(&session, f.catalogs(), at(AT + 1)?, &cancel)
            .is_err()
    );
    assert!(
        inputs
            .recheck(
                &session,
                f.catalogs(),
                at(AT + 1000)?,
                &Cancellation::default()
            )
            .is_err()
    );
    let mut changed = inputs.bytes().to_vec();
    changed.push(b' ');
    assert!(StreamOwnerInputs::decode(&changed).is_err());
    let mut changed: serde_json::Value = serde_json::from_slice(inputs.bytes())?;
    changed[3] = "00".into();
    assert!(StreamOwnerInputs::decode(&serde_json::to_vec(&changed)?).is_err());
    assert!(
        StreamOwnerInputs::new(&session, b"different profile", &claims, at(AT + 1000)?).is_err()
    );
    assert!(
        StreamOwnerInputs::new(
            &session,
            profile,
            &[claims[0].clone(), claims[0].clone()],
            at(AT + 1000)?
        )
        .is_err()
    );
    assert!(
        StreamOwnerInputs::new(
            &session,
            profile,
            &[claims[0].clone(), claim(8, claims[0].scope())?],
            at(AT + 1000)?
        )
        .is_err()
    );
    assert!(StreamOwnerInputs::new(&session, profile, &[], at(AT + 1000)?).is_err());
    assert!(claim(1, b" [1]").is_err());
    assert!(claim(1, b"{}").is_err());
    assert!(claim(1, &[b' '; 1025]).is_err());
    assert!(StreamOwnerInputs::decode(&vec![b' '; 1_048_577]).is_err());
    assert!(StreamOwnerInputs::new(&session, &vec![0; 65537], &claims, at(AT + 1000)?).is_err());
    assert!(
        StreamOwnerInputs::new(
            &session,
            profile,
            &vec![claims[0].clone(); 129],
            at(AT + 1000)?
        )
        .is_err()
    );
    assert_eq!(format!("{inputs:?}"), "StreamOwnerInputs(..)");
    assert_eq!(format!("{:?}", claims[0]), "StreamNamespaceClaim(..)");
    Ok(())
}
